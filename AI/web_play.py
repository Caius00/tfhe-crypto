"""Spielt Flappy Bird endlos im laufenden Angular-Client (ng serve).

Voraussetzungen:
  1. `npm start` im `client/`-Ordner. Default-URL: http://localhost:4200.
  2. Manuell einen Raum erstellen und den 6-stelligen Code notieren.
  3. Diesen Code per --code übergeben.

Das Skript:
  * öffnet ein Chromium-Fenster
  * tritt dem Raum bei
  * liest pro Frame den Spielzustand aus der FlappyBirdComponent
    via Angular debug API (`window.ng.getComponent`)
  * entscheidet per Heuristik (Default) oder geladenem PPO-Netz (mit --model),
    ob geflappt wird
  * bei Game Over wird der Score automatisch vom Angular-Client FHE-
    verschlüsselt an den Server geschickt; das Skript klickt nur Restart.
  * läuft bis Strg-C
  * loggt jede Runde nach AI/logs/web_scores.csv
"""
from __future__ import annotations

import argparse
import asyncio
import csv
import signal
import sys
import time
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from playwright.async_api import Page, async_playwright

from flappy_env import GRAVITY, MAX_VY, make_observation


LOGS_DIR = Path(__file__).parent / "logs"
SCORE_LOG = LOGS_DIR / "web_scores.csv"


@dataclass
class GameState:
    phase: str
    bx: float
    by: float
    bvy: float
    score: int
    game_over_timer: float
    canvas_w: float
    canvas_h: float
    pipes: list[dict]


READ_STATE_JS = """
() => {
  const host = document.querySelector('app-flappy-bird');
  if (!host || !window.ng) return null;
  const comp = window.ng.getComponent(host);
  if (!comp) return null;
  const canvas = host.querySelector('canvas');
  const w = canvas ? canvas.width  : 0;
  const h = canvas ? canvas.height : 0;
  const pipes = (comp.pipes || []).map(p => ({x: p.x, gapY: p.gapY, scored: p.scored}));
  return {
    phase: comp.phase,
    bx: comp.bx,
    by: comp.by,
    bvy: comp.bvy,
    score: comp.score,
    game_over_timer: comp.gameOverTimer,
    canvas_w: w,
    canvas_h: h,
    pipes: pipes,
  };
}
"""


# ---------------------------------------------------------------------------
# Policies
# ---------------------------------------------------------------------------
def _simulate_freefall(by: float, bvy: float, duration: float) -> float:
    y, vy = by, bvy
    step = 1.0 / 60.0
    remaining = duration
    while remaining > 0:
        dt = min(step, remaining)
        vy = min(vy + GRAVITY * dt, MAX_VY)
        y += vy * dt
        remaining -= dt
    return y


def heuristic_should_flap(s: GameState) -> bool:
    """Einfache Physik-Heuristik als Fallback (kein --model übergeben)."""
    visible = sorted(
        (p for p in s.pipes if p["x"] + 26 >= s.bx),
        key=lambda p: p["x"],
    )
    target = visible[0]["gapY"] + 60 if visible else s.canvas_h * 0.55
    y = _simulate_freefall(s.by, s.bvy, 0.10)
    return y > target


class ModelPolicy:
    """Lädt einen PPO-ActorCritic-Checkpoint (`models/best.pt` o.ä.).

    PPO-Policies bleiben oft stochastisch (p_flap ≈ 0.3); deshalb sampelt der
    Browser standardmäßig dieselbe Distribution wie das Training. Argmax
    versagt typischerweise (flapt nie). Der globale flap_cooldown in der
    Mainloop verhindert, dass die Sampling-Variabilität zu Doppelklicks
    innerhalb desselben Frames führt.
    """

    def __init__(self, model_path: Path, device: str = "cpu", deterministic: bool = False):
        import torch

        from train import ActorCritic

        self._torch = torch
        self.deterministic = deterministic
        ckpt = torch.load(model_path, map_location=device, weights_only=False)
        hidden = ckpt.get("hidden", 128)
        self.model = ActorCritic(hidden=hidden).to(device)
        self.model.load_state_dict(ckpt["model"])
        self.model.eval()
        self.device = device

    def should_flap(self, s: GameState) -> bool:
        if s.canvas_w <= 0 or s.canvas_h <= 0:
            return False
        obs = make_observation(s.by, s.bvy, s.bx, s.canvas_w, s.canvas_h, s.pipes)
        obs_t = self._torch.from_numpy(obs).unsqueeze(0).to(self.device)
        with self._torch.no_grad():
            logits, _ = self.model(obs_t)
            if self.deterministic:
                action = int(logits.argmax(dim=-1).item())
            else:
                probs = self._torch.softmax(logits, dim=-1)[0].cpu().numpy()
                action = 1 if np.random.random() < probs[1] else 0
        return action == 1


# ---------------------------------------------------------------------------
# Playwright-Glue
# ---------------------------------------------------------------------------
_stop = False


def _install_signal_handler():
    def _handler(_signum, _frame):
        global _stop
        _stop = True
        print("\nStop angefordert, warte auf saubere Beendigung…")
    signal.signal(signal.SIGINT, _handler)
    signal.signal(signal.SIGTERM, _handler)


async def _click_canvas(page: Page):
    box = await page.evaluate(
        "() => { const c = document.querySelector('app-flappy-bird canvas');"
        " if (!c) return null; const r = c.getBoundingClientRect();"
        " return {x: r.left + r.width/2, y: r.top + r.height/2}; }"
    )
    if box:
        await page.mouse.click(box["x"], box["y"])


async def _join_room(page: Page, url: str, code: str):
    await page.goto(f"{url.rstrip('/')}/leaderboard")
    code_input = page.locator('input[placeholder="6-stelliger Code"]')
    await code_input.wait_for(state="visible", timeout=15_000)
    await code_input.fill(code.upper())
    await page.get_by_role("button", name="BEITRETEN").click()
    await page.wait_for_selector("app-flappy-bird canvas", timeout=15_000)
    await page.wait_for_function(
        "() => { const h = document.querySelector('app-flappy-bird');"
        " if (!h || !window.ng) return false;"
        " const c = window.ng.getComponent(h);"
        " return c && c.phase === 'IDLE'; }",
        timeout=20_000,
    )


async def _read_state(page: Page) -> GameState | None:
    raw = await page.evaluate(READ_STATE_JS)
    if raw is None:
        return None
    return GameState(**raw)


def _log_score(score: int, game_idx: int):
    LOGS_DIR.mkdir(exist_ok=True)
    new_file = not SCORE_LOG.exists()
    with SCORE_LOG.open("a", newline="") as f:
        w = csv.writer(f)
        if new_file:
            w.writerow(["timestamp", "game", "score"])
        w.writerow([time.strftime("%Y-%m-%dT%H:%M:%S"), game_idx, score])


async def run(url: str, code: str, headless: bool, model_path: Path | None,
              device: str, deterministic: bool):
    if model_path:
        mode = "argmax" if deterministic else "sampled"
        print(f"Lade Modell {model_path} (device={device}, mode={mode})")
        policy = ModelPolicy(model_path, device=device, deterministic=deterministic)
        decide = policy.should_flap
    else:
        print("Verwende Heuristik (kein --model übergeben)")
        decide = heuristic_should_flap

    _install_signal_handler()

    async with async_playwright() as pw:
        browser = await pw.chromium.launch(headless=headless, args=["--start-maximized"])
        context = await browser.new_context(no_viewport=True)
        page = await context.new_page()

        print(f"Öffne {url} und trete Raum {code} bei …")
        try:
            await _join_room(page, url, code)
        except Exception as e:
            print(f"Beitritt fehlgeschlagen: {e}")
            await browser.close()
            return

        print("Im Raum. Starte erstes Spiel …")
        await _click_canvas(page)

        game_idx = 0
        last_flap_t = 0.0
        flap_cooldown = 0.07  # Verhindert Doppel-Klicks im selben Frame

        while not _stop:
            try:
                s = await _read_state(page)
            except Exception as e:
                print(f"State-Read-Fehler: {e}")
                break
            if s is None:
                await asyncio.sleep(0.05)
                continue

            now = time.monotonic()

            if s.phase == "PLAYING":
                if decide(s) and (now - last_flap_t) >= flap_cooldown:
                    await _click_canvas(page)
                    last_flap_t = now
                await asyncio.sleep(0.02)
            elif s.phase == "GAME_OVER":
                game_idx += 1
                _log_score(s.score, game_idx)
                print(f"Game {game_idx} Ende — Score {s.score}")
                t0 = time.monotonic()
                while not _stop:
                    s2 = await _read_state(page)
                    if s2 and s2.phase == "GAME_OVER" and s2.game_over_timer > 1.05:
                        break
                    if time.monotonic() - t0 > 5.0:
                        break
                    await asyncio.sleep(0.05)
                if _stop:
                    break
                await _click_canvas(page)
                await asyncio.sleep(0.2)
            else:
                if s.phase == "IDLE":
                    await _click_canvas(page)
                await asyncio.sleep(0.05)

        print(f"Beendet. {game_idx} Spiele. Log: {SCORE_LOG}")
        await browser.close()


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--url", default="http://localhost:4200",
                   help="Basis-URL des ng-serve-Clients")
    p.add_argument("--code", required=True, help="6-stelliger Raum-Code")
    p.add_argument("--headless", action="store_true",
                   help="Chromium unsichtbar laufen lassen")
    p.add_argument("--model", type=Path, default=Path(__file__).parent / "models" / "best.pt",
                   help="Pfad zum PPO-Checkpoint (.pt). Default: models/best.pt")
    p.add_argument("--no-model", action="store_true",
                   help="Modell ignorieren, nur Heuristik nutzen")
    p.add_argument("--device", default="cpu", choices=["cpu", "mps"],
                   help="Inferenz-Device (Default cpu, MPS lohnt bei dem Netz selten)")
    p.add_argument("--deterministic", action="store_true",
                   help="argmax statt sampling. Meist schlechter, weil die PPO-Policy "
                        "stochastisch bleibt. Default ist sampling.")
    args = p.parse_args()

    model_path: Path | None
    if args.no_model:
        model_path = None
    else:
        model_path = args.model
        if not model_path.exists():
            print(f"Modell {model_path} nicht gefunden – fallback auf Heuristik. "
                  f"(--no-model um die Warnung zu unterdrücken)", file=sys.stderr)
            model_path = None

    asyncio.run(run(args.url, args.code, args.headless, model_path, args.device,
                    args.deterministic))


if __name__ == "__main__":
    main()

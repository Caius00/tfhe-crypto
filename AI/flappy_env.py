"""Flappy-Bird-Umgebung, exakt 1:1 zur Client-Komponente.

Alle Konstanten und die Update-Reihenfolge stammen aus
`client/src/app/shared/components/flappy-bird/flappy-bird.component.ts`
(Phase PLAYING). Es gibt zwei Implementierungen:

* `FlappyVecEnv`  – voll vektorisiert über N Envs, NumPy-only. Für Training.
* `make_observation` – baut den Beobachtungsvektor aus rohen Client-Werten.
  Genutzt von `web_play.py`, damit Inference das gleiche Format wie Training hat.

Beobachtungsvektor (6 floats):
    0: by / H                          – Vertikalposition
    1: bvy / MAX_VY                    – Vertikalgeschwindigkeit
    2: (next.x   - bx) / W             – horizontale Distanz zur nächsten Pipe
    3: (next.gapY  - by) / H           – signed Offset zur Lücken-Mitte
    4: (next2.x  - bx) / W             – Distanz zur übernächsten Pipe
    5: (next2.gapY - by) / H           – Offset zur übernächsten Lücken-Mitte

Aktion: 0 = nichts, 1 = flap.
"""

from __future__ import annotations

import numpy as np


# --- Physik-Konstanten 1:1 aus flappy-bird.component.ts ---------------------
PIPE_GAP = 165.0
PIPE_WIDTH = 52.0
PIPE_SPEED = 160.0
PIPE_INTERVAL = 2.0
BIRD_IDLE_X = 90.0
GROUND_H = 58.0
GRAVITY = 600.0
MAX_VY = 520.0
FLAP_VY = -400.0
FLAP_VX_INCR = 45.0
MAX_VX = 90.0
BVX_DECAY_BASE = 0.08            # bvx *= 0.08 ** dt
BIRD_R = 13.0
PIPE_HALF_W = PIPE_WIDTH / 2     # 26
PIPE_CAP_EXTRA = 5.0             # collision: pRight = x + 26 + 5

FPS = 60.0
DT = np.float32(1.0 / FPS)

OBS_DIM = 6
N_ACTIONS = 2

# Max gleichzeitig lebende Pipes pro Env.
# Bei W=1600, PIPE_INTERVAL=2.0, PIPE_SPEED=160 ⇒ Abstand 320 px ⇒ ≤ 6 Pipes.
MAX_PIPES = 6


class FlappyVecEnv:
    """Voll vektorisierte Flappy-Bird-Env in NumPy.

    Alle Operationen laufen batched über die N Envs. Pipes liegen in einem
    fixen Buffer (N, MAX_PIPES) mit `alive`-Maske, sodass keine Python-Listen
    im Hot-Path vorkommen.

    Episoden, die terminieren, werden direkt im selben Step wieder resettet
    (auto-reset), wie es PPO erwartet. Der zurückgegebene `obs` ist *nach*
    dem Reset, `info["episode_return"]` / `info["episode_score"]` enthalten
    bei terminierten Envs die Statistik der gerade beendeten Episode.
    """

    def __init__(
        self,
        n_envs: int = 128,
        randomize_canvas: bool = True,
        w_range: tuple[float, float] = (800.0, 1600.0),
        h_range: tuple[float, float] = (560.0, 900.0),
        canvas_w: float = 1280.0,
        canvas_h: float = 720.0,
        max_steps: int = 20_000,
        seed: int | None = None,
        gap_scale: float = 1.0,
    ):
        self.n_envs = n_envs
        self.randomize_canvas = randomize_canvas
        self.w_range = w_range
        self.h_range = h_range
        self.default_w = float(canvas_w)
        self.default_h = float(canvas_h)
        self.max_steps = max_steps
        # Curriculum: erlaubt es, die Pipe-Lücke während des Trainings zu
        # weiten. Die Inferenz im Browser läuft IMMER mit gap_scale=1.0,
        # daher muss der Trainer den Wert über die Zeit Richtung 1.0 ziehen.
        self.gap_scale = float(gap_scale)
        self._pipe_gap = PIPE_GAP * self.gap_scale

        self.rng = np.random.default_rng(seed)

        # Vogel-State (N,)
        self.bx = np.empty(n_envs, dtype=np.float32)
        self.by = np.empty(n_envs, dtype=np.float32)
        self.bvx = np.empty(n_envs, dtype=np.float32)
        self.bvy = np.empty(n_envs, dtype=np.float32)

        # Pipe-Buffer (N, MAX_PIPES)
        # alive=False ⇒ Slot ungenutzt; x wird dann auf +inf gesetzt, damit
        # Min-Selects ignorieren.
        self.pipe_x = np.full((n_envs, MAX_PIPES), np.inf, dtype=np.float32)
        self.pipe_gap_y = np.zeros((n_envs, MAX_PIPES), dtype=np.float32)
        self.pipe_scored = np.zeros((n_envs, MAX_PIPES), dtype=bool)
        self.pipe_alive = np.zeros((n_envs, MAX_PIPES), dtype=bool)

        self.pipe_timer = np.zeros(n_envs, dtype=np.float32)
        self.score = np.zeros(n_envs, dtype=np.int32)
        self.canvas_w = np.empty(n_envs, dtype=np.float32)
        self.canvas_h = np.empty(n_envs, dtype=np.float32)
        self.episode_steps = np.zeros(n_envs, dtype=np.int32)
        self.episode_return = np.zeros(n_envs, dtype=np.float32)

        self._reset_all()

    # ------------------------------------------------------------------
    # Reset / Spawn
    # ------------------------------------------------------------------
    def _sample_canvas(self, mask: np.ndarray) -> None:
        k = int(mask.sum())
        if k == 0:
            return
        if self.randomize_canvas:
            self.canvas_w[mask] = self.rng.uniform(
                self.w_range[0], self.w_range[1], size=k
            ).astype(np.float32)
            self.canvas_h[mask] = self.rng.uniform(
                self.h_range[0], self.h_range[1], size=k
            ).astype(np.float32)
        else:
            self.canvas_w[mask] = self.default_w
            self.canvas_h[mask] = self.default_h

    def _reset_envs(self, mask: np.ndarray) -> None:
        """Setze die mit `mask` markierten Envs zurück.

        Spiegelt `startGame()` aus dem Client: bx=IDLE_X, by=H*0.45, bvy=0
        und sofort einmal flap()."""
        if not mask.any():
            return
        self._sample_canvas(mask)

        self.bx[mask] = BIRD_IDLE_X
        self.by[mask] = self.canvas_h[mask] * 0.45
        self.bvx[mask] = 0.0
        self.bvy[mask] = 0.0

        # startGame() ruft sofort flap()
        self.bvy[mask] = FLAP_VY
        self.bvx[mask] = np.minimum(self.bvx[mask] + FLAP_VX_INCR, MAX_VX)

        self.pipe_x[mask] = np.inf
        self.pipe_gap_y[mask] = 0.0
        self.pipe_scored[mask] = False
        self.pipe_alive[mask] = False

        self.pipe_timer[mask] = 0.0
        self.score[mask] = 0
        self.episode_steps[mask] = 0
        self.episode_return[mask] = 0.0

    def _reset_all(self) -> None:
        self._reset_envs(np.ones(self.n_envs, dtype=bool))

    # ------------------------------------------------------------------
    # Pipe-Management (vektorisiert)
    # ------------------------------------------------------------------
    def _spawn_pipes(self, dt: float) -> None:
        """Spawnt eine Pipe in jedem Env, dessen pipe_timer ≥ PIPE_INTERVAL ist."""
        self.pipe_timer += dt
        ready = self.pipe_timer >= PIPE_INTERVAL
        if not ready.any():
            return
        self.pipe_timer[ready] = 0.0

        # Index des ersten freien Slots pro Env (argmax über not-alive). Falls
        # alle Slots belegt sind (sollte mit MAX_PIPES=6 nie passieren), nehmen
        # wir Slot 0 – aber via Mask `has_slot` schließen wir das aus.
        free = ~self.pipe_alive  # (N, MAX_PIPES)
        has_slot = free.any(axis=1)
        spawn = ready & has_slot
        if not spawn.any():
            return
        # argmax findet den ersten True-Eintrag in jeder Zeile
        slot_idx = free.argmax(axis=1)  # (N,)

        env_idx = np.where(spawn)[0]
        slots = slot_idx[env_idx]
        H = self.canvas_h[env_idx]
        W = self.canvas_w[env_idx]
        min_gap = 100.0 + self._pipe_gap / 2
        max_gap = H - GROUND_H - 60.0 - self._pipe_gap / 2
        # Falls max_gap ≤ min_gap (extrem schmaler Canvas), nimm Mitte.
        valid = max_gap > min_gap
        gap_y = np.where(
            valid,
            min_gap + self.rng.random(env_idx.size).astype(np.float32) * (max_gap - min_gap),
            (min_gap + max_gap) * 0.5,
        )

        self.pipe_x[env_idx, slots] = W + PIPE_WIDTH
        self.pipe_gap_y[env_idx, slots] = gap_y
        self.pipe_scored[env_idx, slots] = False
        self.pipe_alive[env_idx, slots] = True

    def _move_pipes(self, dt: float) -> None:
        # Lebende Pipes nach links schieben, ansonsten +inf belassen.
        self.pipe_x = np.where(
            self.pipe_alive, self.pipe_x - PIPE_SPEED * dt, np.inf
        ).astype(np.float32, copy=False)
        # Aus dem Bild gelaufen ⇒ Slot freigeben.
        gone = self.pipe_alive & (self.pipe_x <= -PIPE_WIDTH - 10.0)
        if gone.any():
            self.pipe_alive[gone] = False
            self.pipe_x[gone] = np.inf
            self.pipe_scored[gone] = False

    def _update_score(self) -> np.ndarray:
        """Vergibt +1 pro neu passierter Pipe, gibt Score-Inkrement (N,) zurück."""
        bx = self.bx[:, None]  # (N, 1)
        passed = self.pipe_alive & (~self.pipe_scored) & (self.pipe_x + PIPE_HALF_W < bx)
        if not passed.any():
            return np.zeros(self.n_envs, dtype=np.int32)
        inc = passed.sum(axis=1).astype(np.int32)
        self.pipe_scored |= passed
        self.score += inc
        return inc

    def _check_collision(self) -> np.ndarray:
        H = self.canvas_h
        ground_y = H - GROUND_H
        ground_hit = self.by + BIRD_R >= ground_y
        ceiling_hit = self.by - BIRD_R <= 0

        bx = self.bx[:, None]
        by = self.by[:, None]
        p_left = self.pipe_x - PIPE_HALF_W
        p_right = self.pipe_x + PIPE_HALF_W + PIPE_CAP_EXTRA
        gap_top = self.pipe_gap_y - self._pipe_gap / 2
        gap_bottom = self.pipe_gap_y + self._pipe_gap / 2

        horiz = (bx + BIRD_R > p_left) & (bx - BIRD_R < p_right)
        vert = (by - BIRD_R < gap_top) | (by + BIRD_R > gap_bottom)
        pipe_hit = (self.pipe_alive & horiz & vert).any(axis=1)

        return ground_hit | ceiling_hit | pipe_hit

    # ------------------------------------------------------------------
    # Observation
    # ------------------------------------------------------------------
    def _observe(self) -> np.ndarray:
        W = self.canvas_w
        H = self.canvas_h
        bx = self.bx[:, None]

        # Distance to pipes; ignore those whose right edge already left the bird.
        rel_x = self.pipe_x - self.bx[:, None]                 # (N, MAX_PIPES)
        relevant = self.pipe_alive & (self.pipe_x + PIPE_HALF_W >= bx)
        key = np.where(relevant, rel_x, np.inf).astype(np.float32)

        # Zwei kleinste rel_x finden (per partition).
        order = np.argpartition(key, kth=min(1, MAX_PIPES - 1), axis=1)
        first_idx = order[:, 0]
        second_idx = order[:, 1] if MAX_PIPES > 1 else order[:, 0]
        rows = np.arange(self.n_envs)

        dx1 = key[rows, first_idx]
        dx2 = key[rows, second_idx]
        gap1 = self.pipe_gap_y[rows, first_idx]
        gap2 = self.pipe_gap_y[rows, second_idx]

        # Ordnen (das kleinste muss tatsächlich kleiner sein – argpartition
        # garantiert das nur für kth, swap falls nötig).
        swap = dx2 < dx1
        if swap.any():
            dx1[swap], dx2[swap] = dx2[swap], dx1[swap]
            gap1[swap], gap2[swap] = gap2[swap], gap1[swap]

        has1 = np.isfinite(dx1)
        has2 = np.isfinite(dx2)

        dx1_n = np.where(has1, dx1 / W, 1.0).astype(np.float32)
        dy1_n = np.where(has1, (gap1 - self.by) / H, 0.0).astype(np.float32)
        dx2_n = np.where(has2, dx2 / W, 2.0).astype(np.float32)
        dy2_n = np.where(has2, (gap2 - self.by) / H, 0.0).astype(np.float32)

        obs = np.stack(
            [self.by / H, self.bvy / MAX_VY, dx1_n, dy1_n, dx2_n, dy2_n],
            axis=1,
        ).astype(np.float32)
        return obs

    def _nearest_gap_y(self) -> np.ndarray:
        """Y-Koordinate der nächsten relevanten Pipe-Lücke; sonst H*0.5."""
        bx = self.bx[:, None]
        rel_x = self.pipe_x - bx
        relevant = self.pipe_alive & (self.pipe_x + PIPE_HALF_W >= bx)
        key = np.where(relevant, rel_x, np.inf).astype(np.float32)
        first_idx = key.argmin(axis=1)
        rows = np.arange(self.n_envs)
        gap = self.pipe_gap_y[rows, first_idx]
        valid = np.isfinite(key[rows, first_idx])
        return np.where(valid, gap, self.canvas_h * 0.5).astype(np.float32)

    def set_gap_scale(self, scale: float) -> None:
        """Curriculum-Hook: ändert die Pipe-Lückenweite. Wirkt ab der nächsten
        gespawnten Pipe. Inferenz im Browser läuft mit 1.0."""
        self.gap_scale = float(scale)
        self._pipe_gap = PIPE_GAP * self.gap_scale

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------
    def reset(self) -> np.ndarray:
        self._reset_all()
        return self._observe()

    def step(self, action: np.ndarray):
        """action: shape (N,) int. 0 = nichts, 1 = flap."""
        dt = float(DT)

        # 1) Flap
        flap = action.astype(bool)
        if flap.any():
            self.bvy[flap] = FLAP_VY
            self.bvx[flap] = np.minimum(self.bvx[flap] + FLAP_VX_INCR, MAX_VX)

        # 2) Schwerkraft + bvx-Decay + Position
        self.bvy = np.minimum(self.bvy + GRAVITY * dt, MAX_VY).astype(np.float32, copy=False)
        self.bvx = (self.bvx * (BVX_DECAY_BASE ** dt)).astype(np.float32, copy=False)
        self.bx += self.bvx * dt
        self.by += self.bvy * dt
        # X-Clamp wie im Client (50 ≤ bx ≤ W*0.35)
        np.clip(self.bx, 50.0, self.canvas_w * 0.35, out=self.bx)

        # 3) Pipes spawnen + bewegen
        self._spawn_pipes(dt)
        self._move_pipes(dt)

        # 4) Score
        score_inc = self._update_score()

        # 5) Kollision
        collided = self._check_collision()

        # 6) Reward-Shaping (für PPO-Exploration):
        #    – alive bonus: +0.05 pro überlebten Step
        #    – proximity:   bis +0.05 pro Step bei perfekter Gap-Mitte
        #    – score:       +10.0 pro passierter Pipe (klar dominant)
        #    – Tod:         -5.0 (deutlich, damit Sterben sich nicht "lohnt")
        # Die hohe Score-Belohnung sorgt dafür, dass eine einzige gepasste
        # Pipe alles Alive-Bonus-Engineering überstrahlt – damit kollabiert
        # PPO nicht in eine "nie flappen, schwebe Richtung Boden"-Policy.
        gap_y = self._nearest_gap_y()
        proximity = 1.0 - np.minimum(
            np.abs(self.by - gap_y) / (self.canvas_h * 0.5), 1.0
        )
        reward = (0.05 + 0.05 * proximity + 10.0 * score_inc.astype(np.float32)).astype(np.float32)
        reward = np.where(collided, -5.0, reward).astype(np.float32)

        self.episode_steps += 1
        self.episode_return += reward
        truncated = self.episode_steps >= self.max_steps
        done = collided | truncated

        info = {
            "score": self.score.copy(),
            "episode_return": np.where(done, self.episode_return, 0.0).copy(),
            "episode_score": np.where(done, self.score, 0).copy(),
            "episode_steps": np.where(done, self.episode_steps, 0).copy(),
            "done": done.copy(),
        }

        # Auto-Reset für terminierte Envs
        if done.any():
            self._reset_envs(done)

        obs = self._observe()
        return obs, reward, done, info


# ---------------------------------------------------------------------------
# Inferenz-Helfer für web_play.py
# ---------------------------------------------------------------------------
def make_observation(
    by: float,
    bvy: float,
    bx: float,
    canvas_w: float,
    canvas_h: float,
    pipes: list[dict],
) -> np.ndarray:
    """Baut denselben 6-D-Beobachtungsvektor aus rohen Client-Werten."""
    relevant = [p for p in pipes if p["x"] + PIPE_HALF_W >= bx]
    relevant.sort(key=lambda p: p["x"])
    if relevant:
        p1 = relevant[0]
        dx1 = (p1["x"] - bx) / canvas_w
        dy1 = (p1["gapY"] - by) / canvas_h
    else:
        dx1, dy1 = 1.0, 0.0
    if len(relevant) >= 2:
        p2 = relevant[1]
        dx2 = (p2["x"] - bx) / canvas_w
        dy2 = (p2["gapY"] - by) / canvas_h
    else:
        dx2, dy2 = 2.0, 0.0
    return np.array(
        [by / canvas_h, bvy / MAX_VY, dx1, dy1, dx2, dy2],
        dtype=np.float32,
    )

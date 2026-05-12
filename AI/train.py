"""PPO-Trainer für Flappy Bird, voll vektorisiert, optimiert für Apple Silicon.

Designentscheidungen für Mac M5:

* Env läuft komplett in NumPy (siehe `flappy_env.FlappyVecEnv`) – kein Python-
  Loop über Envs, keine Subprocesses.
* PPO ist Custom-Code (~250 Zeilen): keine SB3-/VecEnv-Wrapper-Schicht.
* Default-Device = CPU, weil das Netz winzig ist (≈ 17k Parameter) und MPS für
  solche Größen vom Tensor-Transfer dominiert wird. Per `--device mps` kann
  trotzdem MPS gewählt werden.
* 128 parallele Envs × 256 Steps = 32 768 Transitions pro Rollout. Reicht für
  stabile PPO-Updates und maximiert NumPy-Auslastung.
"""
from __future__ import annotations

import argparse
import math
import time
from pathlib import Path

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F
from torch.distributions import Categorical

from flappy_env import FlappyVecEnv, GRAVITY, MAX_VY, N_ACTIONS, OBS_DIM


MODELS_DIR = Path(__file__).parent / "models"
LOGS_DIR = Path(__file__).parent / "logs"


# ---------------------------------------------------------------------------
# Netz
# ---------------------------------------------------------------------------
class ActorCritic(nn.Module):
    """Shared-Trunk-MLP mit getrenntem Policy- und Value-Head."""

    def __init__(self, obs_dim: int = OBS_DIM, n_actions: int = N_ACTIONS, hidden: int = 128):
        super().__init__()
        self.trunk = nn.Sequential(
            nn.Linear(obs_dim, hidden),
            nn.Tanh(),
            nn.Linear(hidden, hidden),
            nn.Tanh(),
        )
        self.policy_head = nn.Linear(hidden, n_actions)
        self.value_head = nn.Linear(hidden, 1)

        # Orthogonale Init mit kleiner Policy-Skala – klassische PPO-Praxis.
        for m in self.trunk.modules():
            if isinstance(m, nn.Linear):
                nn.init.orthogonal_(m.weight, math.sqrt(2))
                nn.init.zeros_(m.bias)
        nn.init.orthogonal_(self.policy_head.weight, 0.01)
        # Bias initial Richtung Flap (~p_flap ≈ 0.62). Sonst stürzt der Vogel
        # in den ersten 2s ab, bevor er je eine Pipe sieht – PPO konvergiert
        # dann gerne zu einer Policy mit p_flap < 0.5, die in argmax-Inferenz
        # NIE flappt und am Boden stirbt.
        with torch.no_grad():
            self.policy_head.bias.copy_(torch.tensor([0.0, 0.5]))
        nn.init.orthogonal_(self.value_head.weight, 1.0)
        nn.init.zeros_(self.value_head.bias)

    def forward(self, obs: torch.Tensor) -> tuple[torch.Tensor, torch.Tensor]:
        h = self.trunk(obs)
        return self.policy_head(h), self.value_head(h).squeeze(-1)

    @torch.no_grad()
    def act(self, obs: torch.Tensor, deterministic: bool = False):
        logits, value = self.forward(obs)
        if deterministic:
            action = logits.argmax(dim=-1)
            log_prob = F.log_softmax(logits, dim=-1).gather(-1, action[:, None]).squeeze(-1)
        else:
            dist = Categorical(logits=logits)
            action = dist.sample()
            log_prob = dist.log_prob(action)
        return action, log_prob, value


# ---------------------------------------------------------------------------
# GAE
# ---------------------------------------------------------------------------
def compute_gae(
    rewards: np.ndarray,      # (T, N)
    values: np.ndarray,       # (T, N)
    dones: np.ndarray,        # (T, N)
    last_value: np.ndarray,   # (N,)
    gamma: float,
    lam: float,
) -> tuple[np.ndarray, np.ndarray]:
    T, N = rewards.shape
    advantages = np.zeros((T, N), dtype=np.float32)
    gae = np.zeros(N, dtype=np.float32)
    next_value = last_value.astype(np.float32)
    next_nonterminal = np.ones(N, dtype=np.float32)
    for t in reversed(range(T)):
        delta = rewards[t] + gamma * next_value * next_nonterminal - values[t]
        gae = delta + gamma * lam * next_nonterminal * gae
        advantages[t] = gae
        next_value = values[t]
        next_nonterminal = 1.0 - dones[t].astype(np.float32)
    returns = advantages + values
    return advantages, returns


# ---------------------------------------------------------------------------
# Device-Auswahl
# ---------------------------------------------------------------------------
def heuristic_action(env: FlappyVecEnv, look_ahead: float = 0.10,
                     offset_px: float = 30.0) -> np.ndarray:
    """Vektorisierte Physik-Heuristik (flap genau dann, wenn der Vogel sonst
    in look_ahead Sekunden unter sein Ziel fällt).

    Ziel: Mitte der nächsten Pipe-Lücke (oder Canvas-Mitte, wenn keine
    Pipe sichtbar). Der Offset kompensiert den Aufwärtsschub jedes Flaps –
    der Vogel pendelt um (target - offset)."""
    by = env.by
    bvy = env.bvy
    H = env.canvas_h
    # Predict by in look_ahead seconds without flapping (using kinematic eq):
    bvy_future = np.minimum(bvy + GRAVITY * look_ahead, MAX_VY)
    avg_bvy = (bvy + bvy_future) * 0.5
    y_future = by + avg_bvy * look_ahead

    gap_y = env._nearest_gap_y()
    target = gap_y + offset_px  # leicht unter gap_y (Schwingung mittelt sich nach oben)
    return (y_future > target).astype(np.int32)


def behavior_clone_pretrain(model: ActorCritic, env: FlappyVecEnv,
                            n_samples: int, n_epochs: int, device: torch.device,
                            batch_size: int = 1024, lr: float = 1e-3) -> None:
    """Sammelt (obs, action) per Heuristik und trainiert das Policy-Netz mit
    Cross-Entropy. Damit hat PPO einen sinnvollen Startpunkt, anstatt zufällig
    zu flappen und am Boden/Decke zu sterben, bevor je eine Pipe gespawnt ist.
    """
    print(f"[bc] sammle {n_samples:,} Heuristik-Transitions …")
    obs_buf = np.zeros((n_samples, OBS_DIM), dtype=np.float32)
    act_buf = np.zeros(n_samples, dtype=np.int64)
    obs = env.reset()
    n = 0
    while n < n_samples:
        action = heuristic_action(env)
        chunk = min(env.n_envs, n_samples - n)
        obs_buf[n:n + chunk] = obs[:chunk]
        act_buf[n:n + chunk] = action[:chunk]
        n += chunk
        obs, _, _, _ = env.step(action)
    print(f"[bc] flap-Anteil: {act_buf.mean():.3f}")

    obs_t = torch.from_numpy(obs_buf).to(device)
    act_t = torch.from_numpy(act_buf).to(device)
    opt = torch.optim.Adam(model.parameters(), lr=lr)
    for epoch in range(n_epochs):
        idx = np.arange(n_samples)
        np.random.shuffle(idx)
        total_loss = 0.0
        correct = 0
        for start in range(0, n_samples, batch_size):
            mb = torch.from_numpy(idx[start:start + batch_size]).to(device)
            logits, _ = model(obs_t[mb])
            loss = F.cross_entropy(logits, act_t[mb])
            opt.zero_grad(set_to_none=True)
            loss.backward()
            opt.step()
            total_loss += float(loss) * mb.size(0)
            correct += int((logits.argmax(-1) == act_t[mb]).sum())
        acc = correct / n_samples
        print(f"[bc] epoch {epoch + 1}/{n_epochs}  loss={total_loss / n_samples:.4f}  acc={acc:.3f}")
    # Reset Env-Statistik nach BC-Sampling
    env._reset_all()


def pick_device(name: str | None) -> torch.device:
    if name == "cpu":
        return torch.device("cpu")
    if name == "mps":
        if not torch.backends.mps.is_available():
            raise SystemExit("MPS angefordert, aber nicht verfügbar.")
        return torch.device("mps")
    # auto: bei dem winzigen Netz schlägt CPU MPS meistens, also Default CPU.
    return torch.device("cpu")


# ---------------------------------------------------------------------------
# Eval
# ---------------------------------------------------------------------------
@torch.no_grad()
def evaluate(model: ActorCritic, device: torch.device, n_envs: int = 16,
             max_steps: int = 10_000, deterministic: bool = False) -> tuple[float, int]:
    """Eval auf festem Canvas. Default = sampled, weil PPO-Policies oft
    stochastisch bleiben (p_flap ≈ 0.3) und argmax dann nie flappt → 0 Score.
    Im Browser sampelt web_play.py ebenfalls."""
    env = FlappyVecEnv(
        n_envs=n_envs,
        randomize_canvas=False,
        canvas_w=1280.0,
        canvas_h=720.0,
        max_steps=max_steps,
        seed=12345,
    )
    obs = env.reset()
    done_any = np.zeros(n_envs, dtype=bool)
    final_score = np.zeros(n_envs, dtype=np.int32)
    steps = 0
    while not done_any.all() and steps < max_steps:
        obs_t = torch.from_numpy(obs).to(device)
        action, _, _ = model.act(obs_t, deterministic=deterministic)
        action_np = action.detach().cpu().numpy().astype(np.int32)
        obs, _, done, info = env.step(action_np)
        new_done = done & ~done_any
        if new_done.any():
            final_score[new_done] = info["episode_score"][new_done]
            done_any |= new_done
        steps += 1
    # Envs, die nie terminiert sind, mit aktuellem Score auswerten
    if not done_any.all():
        final_score[~done_any] = env.score[~done_any]
    return float(final_score.mean()), int(final_score.max())


# ---------------------------------------------------------------------------
# PPO-Trainingsloop
# ---------------------------------------------------------------------------
def train(args):
    MODELS_DIR.mkdir(exist_ok=True)
    LOGS_DIR.mkdir(exist_ok=True)

    device = pick_device(args.device)
    print(f"[setup] device={device}  n_envs={args.n_envs}  n_steps={args.n_steps}  "
          f"batch={args.n_envs * args.n_steps}  total={args.total_timesteps:,}")

    torch.manual_seed(args.seed)
    np.random.seed(args.seed)

    env = FlappyVecEnv(
        n_envs=args.n_envs,
        randomize_canvas=True,
        max_steps=args.episode_max_steps,
        seed=args.seed,
        gap_scale=args.gap_scale_start,
    )
    obs = env.reset()

    model = ActorCritic(hidden=args.hidden).to(device)
    if args.resume:
        sd = torch.load(args.resume, map_location=device)
        model.load_state_dict(sd["model"])
        print(f"[setup] resumed from {args.resume}")

    # Behavior-Cloning-Pretraining auf Heuristik – gibt PPO einen Startpunkt,
    # an dem der Vogel zumindest am Leben bleibt und gelegentlich Pipes
    # passiert. Ohne das kollabiert PPO auf Sparse-Reward-Problemen.
    if not args.resume and args.bc_samples > 0:
        behavior_clone_pretrain(
            model, env,
            n_samples=args.bc_samples,
            n_epochs=args.bc_epochs,
            device=device,
            batch_size=args.bc_batch,
            lr=args.bc_lr,
        )
        obs = env.reset()

    optimizer = torch.optim.Adam(model.parameters(), lr=args.lr, eps=1e-5)

    n_iters = args.total_timesteps // (args.n_envs * args.n_steps)
    print(f"[setup] {n_iters} PPO-Iterationen geplant")

    # Rollout-Buffer auf CPU als NumPy halten – wir konvertieren nur in Batches.
    rollout_obs = np.zeros((args.n_steps, args.n_envs, OBS_DIM), dtype=np.float32)
    rollout_actions = np.zeros((args.n_steps, args.n_envs), dtype=np.int64)
    rollout_logprobs = np.zeros((args.n_steps, args.n_envs), dtype=np.float32)
    rollout_values = np.zeros((args.n_steps, args.n_envs), dtype=np.float32)
    rollout_rewards = np.zeros((args.n_steps, args.n_envs), dtype=np.float32)
    rollout_dones = np.zeros((args.n_steps, args.n_envs), dtype=bool)

    ep_scores: list[int] = []     # gleitendes Fenster über alle terminierten Episoden
    ep_returns: list[float] = []
    best_eval_mean = -math.inf
    best_eval_max = 0
    global_step = 0
    t_start = time.time()
    csv_path = LOGS_DIR / "train.csv"
    csv_new = not csv_path.exists()
    csv_f = csv_path.open("a", buffering=1)
    if csv_new:
        csv_f.write("iter,timesteps,wall_s,sps,ep_mean_score,ep_max_score,ep_mean_return,policy_loss,value_loss,entropy,lr\n")

    for it in range(1, n_iters + 1):
        # LR linear decayen
        frac = 1.0 - (it - 1) / max(n_iters, 1)
        cur_lr = args.lr * frac if args.lr_decay else args.lr
        for g in optimizer.param_groups:
            g["lr"] = cur_lr

        # Curriculum: gap_scale linear von start → 1.0 über die ersten
        # `gap_scale_anneal_frac` der Iterationen.
        progress = (it - 1) / max(n_iters, 1)
        anneal_p = min(progress / max(args.gap_scale_anneal_frac, 1e-6), 1.0)
        cur_gap = args.gap_scale_start + (1.0 - args.gap_scale_start) * anneal_p
        env.set_gap_scale(cur_gap)

        # ----- Rollout -----
        for t in range(args.n_steps):
            rollout_obs[t] = obs
            obs_t = torch.from_numpy(obs).to(device)
            action, log_prob, value = model.act(obs_t, deterministic=False)
            action_np = action.detach().cpu().numpy().astype(np.int64)
            log_prob_np = log_prob.detach().cpu().numpy().astype(np.float32)
            value_np = value.detach().cpu().numpy().astype(np.float32)

            obs, reward, done, info = env.step(action_np.astype(np.int32))

            rollout_actions[t] = action_np
            rollout_logprobs[t] = log_prob_np
            rollout_values[t] = value_np
            rollout_rewards[t] = reward
            rollout_dones[t] = done

            if done.any():
                ep_scores.extend(int(s) for s in info["episode_score"][done])
                ep_returns.extend(float(r) for r in info["episode_return"][done])
                # Fenster begrenzen
                if len(ep_scores) > 1000:
                    ep_scores = ep_scores[-1000:]
                    ep_returns = ep_returns[-1000:]

        # Falls keine Episode im Rollout terminiert ist (Modell wird gut),
        # report wenigstens den aktuellen Score-Snapshot, sonst zeigt das
        # Log dauerhaft 0.
        cur_scores_snapshot = env.score.copy()

        global_step += args.n_steps * args.n_envs

        # Bootstrap: Wert der aktuellen obs für GAE-last-value.
        with torch.no_grad():
            obs_t = torch.from_numpy(obs).to(device)
            _, _, last_value = model.act(obs_t, deterministic=False)
            last_value_np = last_value.detach().cpu().numpy().astype(np.float32)

        advantages, returns = compute_gae(
            rollout_rewards, rollout_values, rollout_dones, last_value_np,
            gamma=args.gamma, lam=args.gae_lambda,
        )

        # ----- PPO-Update -----
        b_obs = torch.from_numpy(rollout_obs.reshape(-1, OBS_DIM)).to(device)
        b_actions = torch.from_numpy(rollout_actions.reshape(-1)).to(device)
        b_old_logp = torch.from_numpy(rollout_logprobs.reshape(-1)).to(device)
        b_returns = torch.from_numpy(returns.reshape(-1)).to(device)
        b_values = torch.from_numpy(rollout_values.reshape(-1)).to(device)
        b_adv = torch.from_numpy(advantages.reshape(-1)).to(device)
        # Advantage-Norm
        b_adv = (b_adv - b_adv.mean()) / (b_adv.std() + 1e-8)

        n_samples = b_obs.shape[0]
        mb_size = args.minibatch
        idx = np.arange(n_samples)
        last_loss = (0.0, 0.0, 0.0)  # policy, value, entropy

        for _ in range(args.n_epochs):
            np.random.shuffle(idx)
            for start in range(0, n_samples, mb_size):
                end = start + mb_size
                mb = torch.from_numpy(idx[start:end]).to(device)

                logits, value_pred = model(b_obs[mb])
                dist = Categorical(logits=logits)
                new_logp = dist.log_prob(b_actions[mb])
                entropy = dist.entropy().mean()

                ratio = (new_logp - b_old_logp[mb]).exp()
                adv = b_adv[mb]
                unclipped = ratio * adv
                clipped = torch.clamp(ratio, 1.0 - args.clip, 1.0 + args.clip) * adv
                policy_loss = -torch.min(unclipped, clipped).mean()

                v_clipped = b_values[mb] + torch.clamp(
                    value_pred - b_values[mb], -args.clip, args.clip
                )
                v_loss_un = (value_pred - b_returns[mb]).pow(2)
                v_loss_cl = (v_clipped - b_returns[mb]).pow(2)
                value_loss = 0.5 * torch.max(v_loss_un, v_loss_cl).mean()

                loss = policy_loss + args.vf_coef * value_loss - args.ent_coef * entropy

                optimizer.zero_grad(set_to_none=True)
                loss.backward()
                nn.utils.clip_grad_norm_(model.parameters(), args.max_grad_norm)
                optimizer.step()

                last_loss = (
                    float(policy_loss.detach()),
                    float(value_loss.detach()),
                    float(entropy.detach()),
                )

        # ----- Logging -----
        wall = time.time() - t_start
        sps = int(global_step / max(wall, 1e-6))
        # Episode-Statistik (terminierte Episoden) ODER Live-Snapshot
        if ep_scores:
            ep_mean_score = float(np.mean(ep_scores))
            ep_max_score = int(np.max(ep_scores))
            ep_mean_return = float(np.mean(ep_returns))
        else:
            ep_mean_score = float(cur_scores_snapshot.mean())
            ep_max_score = int(cur_scores_snapshot.max())
            ep_mean_return = 0.0
        # Zusätzlich: aktueller Live-Score-Snapshot über alle Envs
        live_mean = float(cur_scores_snapshot.mean())
        live_max = int(cur_scores_snapshot.max())

        if it % args.log_every == 0 or it == 1 or it == n_iters:
            print(
                f"[iter {it:>4}/{n_iters}] "
                f"steps={global_step:>10,}  "
                f"sps={sps:>6,}  "
                f"ep={ep_mean_score:6.1f}/{ep_max_score:>4}  "
                f"live={live_mean:6.1f}/{live_max:>4}  "
                f"ret={ep_mean_return:7.2f}  "
                f"pi={last_loss[0]:+.3f}  v={last_loss[1]:.3f}  "
                f"H={last_loss[2]:.3f}  gap={cur_gap:.2f}"
            )
        csv_f.write(
            f"{it},{global_step},{wall:.2f},{sps},{ep_mean_score:.4f},{ep_max_score},"
            f"{ep_mean_return:.4f},{last_loss[0]:.6f},{last_loss[1]:.6f},"
            f"{last_loss[2]:.6f},{cur_lr:.6e}\n"
        )

        # Periodische Eval + Best-Model speichern
        if it % args.eval_every == 0 or it == n_iters:
            ev_mean, ev_max = evaluate(model, device, n_envs=args.eval_envs,
                                       max_steps=args.eval_max_steps)
            print(f"        [eval] mean={ev_mean:.2f}  max={ev_max}")
            if ev_mean > best_eval_mean:
                best_eval_mean = ev_mean
                best_eval_max = ev_max
                torch.save(
                    {"model": model.state_dict(),
                     "eval_mean": ev_mean, "eval_max": ev_max,
                     "hidden": args.hidden},
                    MODELS_DIR / "best.pt",
                )
                print(f"        [eval] neues bestes Modell gespeichert (mean={ev_mean:.2f}, max={ev_max})")

    csv_f.close()
    torch.save(
        {"model": model.state_dict(), "hidden": args.hidden},
        MODELS_DIR / "final.pt",
    )
    print(f"[done] final → models/final.pt   best (eval mean) → models/best.pt"
          f"   eval_mean={best_eval_mean:.2f}  eval_max={best_eval_max}")


def parse_args():
    p = argparse.ArgumentParser()
    # Skala
    p.add_argument("--total-timesteps", type=int, default=2_000_000,
                   help="Gesamte Trainings-Steps über alle Envs (Default 2M).")
    p.add_argument("--n-envs", type=int, default=128)
    p.add_argument("--n-steps", type=int, default=256,
                   help="Rollout-Länge pro Env, bevor PPO-Update startet.")
    p.add_argument("--episode-max-steps", type=int, default=20_000)
    # Netz
    p.add_argument("--hidden", type=int, default=128)
    # Optimierer / PPO
    p.add_argument("--lr", type=float, default=3e-4)
    p.add_argument("--lr-decay", action="store_true", default=True)
    p.add_argument("--no-lr-decay", dest="lr_decay", action="store_false")
    p.add_argument("--gamma", type=float, default=0.999)
    p.add_argument("--gae-lambda", type=float, default=0.95)
    p.add_argument("--clip", type=float, default=0.2)
    p.add_argument("--ent-coef", type=float, default=0.03,
                   help="Höherer Default als üblich, damit PPO nicht früh in "
                        "ein 'nie flappen'-Lokal-Optimum kollabiert.")
    p.add_argument("--vf-coef", type=float, default=0.5)
    p.add_argument("--max-grad-norm", type=float, default=0.5)
    p.add_argument("--n-epochs", type=int, default=10)
    p.add_argument("--minibatch", type=int, default=1024)
    # Behavior-Cloning-Pretraining
    p.add_argument("--bc-samples", type=int, default=50_000,
                   help="Anzahl (obs, action)-Paare per Heuristik vor PPO. 0 = überspringen.")
    p.add_argument("--bc-epochs", type=int, default=5)
    p.add_argument("--bc-batch", type=int, default=1024)
    p.add_argument("--bc-lr", type=float, default=1e-3)
    # Curriculum
    p.add_argument("--gap-scale-start", type=float, default=1.7,
                   help="Pipe-Lücke initial * Original. Verhindert, dass PPO "
                        "in 'nie flappen' kollabiert, bevor je eine Pipe gescored wird.")
    p.add_argument("--gap-scale-anneal-frac", type=float, default=0.5,
                   help="Anteil der Iterationen, über die die Lücke linear auf 1.0 schrumpft.")
    # Device
    p.add_argument("--device", choices=["cpu", "mps", "auto"], default="auto",
                   help="auto = CPU (für dieses winzige Netz schneller als MPS).")
    # Misc
    p.add_argument("--seed", type=int, default=0)
    p.add_argument("--log-every", type=int, default=1)
    p.add_argument("--eval-every", type=int, default=10)
    p.add_argument("--eval-envs", type=int, default=16)
    p.add_argument("--eval-max-steps", type=int, default=10_000)
    p.add_argument("--resume", type=str, default=None)
    return p.parse_args()


if __name__ == "__main__":
    train(parse_args())

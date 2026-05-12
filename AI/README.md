# flappy-rl

PPO-Agent für das Flappy-Bird-Spiel des Angular-Clients.

## Training

```sh
uv run train.py --total-timesteps 5000000
```

Speichert nach Default `models/best.pt` (laut Eval-Mean) und `models/final.pt`.

## Spielen im Browser

Vorher in `client/`: `npm start`, Raum erstellen, 6-stelligen Code notieren.

```sh
uv run web_play.py --code XXXXXX
```

Lädt automatisch `models/best.pt` und spielt endlos bis Strg-C.

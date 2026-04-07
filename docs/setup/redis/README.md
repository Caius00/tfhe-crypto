# Redis

**URL:** `redis://localhost:6379`

| Variable | Wert |
|----------|------|
| Host | `localhost` |
| Port | `6379` |

**Starten**
```bash
cd docs/setup/redis
docker-compose up -d
```

**Stoppen**
```bash
cd docs/setup/redis
docker-compose down
```

**Löschen** (inkl. Daten)
```bash
cd docs/setup/redis
docker-compose down -v
```

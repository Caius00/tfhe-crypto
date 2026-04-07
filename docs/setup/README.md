# Docker Setup (Mini)

Kurzanleitung fuer Docker + Compose auf macOS, Windows und Linux.

## 1) Installieren

### macOS
- Docker Desktop installieren: https://www.docker.com/products/docker-desktop/
- Alternative (leichtgewichtig): Colima + Docker CLI

### Windows
- Docker Desktop installieren: https://www.docker.com/products/docker-desktop/
- WSL2 aktivieren (falls abgefragt)

### Linux (Ubuntu/Debian)
```bash
sudo apt-get update
sudo apt-get install -y docker.io docker-compose-v2
```

## 2) Verifizieren
```bash
docker --version
docker compose version
```

Wenn `docker compose` nicht verfuegbar ist, pruefe Legacy:
```bash
docker-compose --version
```

## 3) Im Projekt nutzen
```bash
cd docs/setup/redis
docker-compose up -d
```

Weitere Setups:
- `docs/setup/redis/README.md`
- `docs/setup/postgres/README.md`


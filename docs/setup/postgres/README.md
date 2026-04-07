# PostgreSQL

**URL:** `postgres://use:password@localhost:5432/mydb`

| Variable | Wert |
|----------|------|
| User | `use` |
| Password | `password` |
| Database | `mydb` |
| Port | `5432` |

**Starten**
```bash
cd docs/setup/postgres
docker-compose up -d
```

**Stoppen**
```bash
cd docs/setup/postgres
docker-compose down
```

**Löschen** (inkl. Daten)
```bash
cd docs/setup/postgres
docker-compose down -v
```

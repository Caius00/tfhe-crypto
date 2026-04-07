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
docker compose up -d
```

**Stoppen**
```bash
docker compose down
```

**Löschen** (inkl. Daten)
```bash
docker compose down -v
```

# Service containerisieren

Ein gemeinsames `Dockerfile` im Root für alle Services — Service-Name als Build-Arg übergeben.

**Bauen**
```bash
docker build --build-arg SERVICE_NAME=encrypted-key-value-store -t encrypted-key-value-store .
```

**Starten**
```bash
docker run --rm encrypted-key-value-store
```

**Alle Service-Namen**

| Ordner | SERVICE_NAME |
|--------|-------------|
| `01-encrypted-key-value-store` | `encrypted-key-value-store` |
| `02-encrypted-age-verification` | `encrypted-age-verification` |
| `03-encrypted-voting-polling` | `encrypted-voting-polling` |
| `04-sealed-bid-auction` | `sealed-bid-auction` |
| `05-encrypted-statistics-service` | `encrypted-statistics-service` |
| `06-encrypted-genomics` | `encrypted-genomics` |
| `07-encrypted-image-processing` | `encrypted-image-processing` |
| `08-encrypted-leaderboard` | `encrypted-leaderboard` |
| `09-encrypted-program-execution` | `encrypted-program-execution` |

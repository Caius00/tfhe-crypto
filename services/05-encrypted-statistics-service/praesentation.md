# UC5 – Encrypted Statistics Service
## Präsentationsnotizen

---

## Was macht UC5?

Sechs Statistiken über eine Zahlenliste — **Server sieht nie die echten Werte**:
Summe · Anzahl · Min · Max · Durchschnitt · Median

**Ablauf:**
1. Browser generiert Schlüsselpaar (ClientKey bleibt im Browser, verlässt ihn nie)
2. ServerKey einmalig hochladen → Server gibt Session-ID zurück (~80 MB, einmalig)
3. Browser verschlüsselt jeden Wert einzeln
4. Verschlüsselte Liste + Session-ID → Server rechnet homomorph
5. Browser entschlüsselt Ergebnisse lokal mit dem ClientKey

---

## Anforderungen

- Server darf **nie** Klartext sehen — weder Eingaben noch Ergebnisse
- Alle sechs Kennzahlen korrekt, auch mit negativen Zahlen
- Parallelisierung nötig — FHE ist um Größenordnungen langsamer als normale Arithmetik
- Median braucht Sortierung **ohne Klartext-Vergleiche** — sonst würde der Server aus dem Vergleichsergebnis Informationen über die Werte ableiten können

---

## Lösungen

### Auto-Bitbreite
Browser wählt die kleinstmögliche Bitbreite anhand von Min/Max der Eingabe:
- alle Werte in [-128, 127] → **Int8**
- alle Werte in [-32.768, 32.767] → **Int16**
- sonst → **Int32**

Kleinere Bitbreite = deutlich schnellere Berechnung (TFHE-Kosten skalieren mit Bitbreite).

Tradeoff: `bit_width` verrät dem Server die Größenordnung der Werte → bewusst akzeptiert.

### Parallelisierung mit Rayon
Summe, Min, Max, Durchschnitt: `par_iter().reduce_with()` → binärer Reduce-Baum, **O(log n)** Tiefe statt O(n).

Min/Max-Vergleich homomorph: `.lt()` / `.gt()` → verschlüsselter Bool → `if_then_else` wählt Ergebnis aus, ohne dass der Server weiß welcher Wert größer war.

### Median – Batcher Odd-Even Mergesort
Problem: Quicksort/Mergesort trifft Entscheidungen anhand von Vergleichsergebnissen → auf FHE nicht erlaubt.

Lösung: Sortiernetzwerk bei dem **alle Vergleiche vor der Ausführung feststehen** — unabhängig von den Werten.

- Netzwerk besteht aus Runden; innerhalb einer Runde alle Paare parallel (Rayon)
- **O(log²n)** sequentielle Runden
- Korrektheit: 0/1-Prinzip (Knuth TAOCP) — wer alle Bitmuster korrekt sortiert, sortiert alles korrekt

### FheEngine pro Session
TFHE-rs braucht den ServerKey auf jedem Rechenthread gesetzt.

Lösung: jede Session bekommt einen eigenen Rayon-Pool, dessen Threads beim Start den Key setzen → parallele Sessions mit verschiedenen Keys stören sich nicht.

Key wird einmalig bei `POST /session` dekomprimiert und gecacht — alle Berechnungsrequests dieser Session nutzen dieselbe Engine.

FHE-Operationen laufen in `block_in_place` → Tokio-Runtime wird nicht blockiert.

### Generics + DivideByElementCount-Trait
Drei Bitbreiten (Int8/16/32), aber dieselbe Logik für alle.

Problem: TFHE-rs hat keine generische Division — `FheInt8 / i8`, `FheInt16 / i16` usw. sind separat.

Lösung: eigenes `DivideByElementCount`-Trait → eine Funktion statt dreimal Copy-Paste.

---

## Was weiß der Server?

| | sichtbar? |
|---|---|
| Anzahl der Werte (n) | ja |
| Größenordnung der Werte | ja (über `bit_width`) |
| Die konkreten Werte | **nein** |
| Summe, Min, Max, Avg, Median | **nein** |
| ClientKey | **nein** — verlässt den Browser nie |
| ServerKey | ja — einmalig hochgeladen, erlaubt nur rechnen, nicht entschlüsseln |

---

## Performance (Netcup-Server, 1 paralleler Nutzer)

| Konfiguration | 95% der Requests unter |
|---|---|
| n=5, Int8 | 2,23 s |
| n=10, Int8 | 4,46 s |
| n=10, Int16 | 7,95 s |
| n=10, Int32 | 16,16 s |

Ohne Session-Caching: +~18 s pro Request (Key-Upload).

**Stresstest** (bis 10 parallele Nutzer): 52% Fehlerrate ab ~2–3 gleichzeitigen Requests → CPU-Engpass, Nginx-Timeout nach 60 s.

---

## Einschränkungen

- **Kein Float** — TFHE-rs hat keine Float-Typen; Durchschnitt ist ganzzahlig (1,5 → 1)
- **bit_width-Leak** — verrät Größenordnung der Werte; bewusst akzeptiert
- **Session-Lebensdauer** — 10 min Idle-Timeout; danach 404, Key muss neu hochgeladen werden
- **Listengröße** — n > ~20 mit Int32 dauert mehrere Minuten; Service ist für kleine Listen konzipiert

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
→ `statistics.component.ts` (`selectOptimalBitWidth`) · `main.rs:251–259` (Dispatch auf bit_width)

Browser wählt die kleinstmögliche Bitbreite anhand von Min/Max der Eingabe:
- alle Werte in [-128, 127] → **Int8**
- alle Werte in [-32.768, 32.767] → **Int16**
- sonst → **Int32**

Kleinere Bitbreite = deutlich schnellere Berechnung (TFHE-Kosten skalieren mit Bitbreite).

Tradeoff: `bit_width` verrät dem Server die Größenordnung der Werte → bewusst akzeptiert.

### Parallelisierung mit Rayon
→ `statistics.rs:44–66`

Summe, Min, Max und Durchschnitt nutzen alle `par_iter().reduce_with()` aus Rayon. Was das bedeutet:

Statt die Liste sequentiell von links nach rechts durchzugehen, baut Rayon intern einen **binären Reduce-Baum**: die Liste wird halbiert, beide Hälften werden parallel bearbeitet, dann werden die Zwischenergebnisse zusammengeführt — und das rekursiv. Ergebnis: **O(log n)** sequentielle Schritte statt O(n).

Bei Summe ist das straightforward — einfach addieren. Bei Min/Max wird es interessanter, weil der Vergleich homomorph laufen muss:
1. `.lt()` / `.gt()` liefert einen **verschlüsselten Bool** (`FheBool`) — der Server weiß nicht ob true oder false
2. `if_then_else` wählt dann homomorph das Ergebnis aus — ohne dass der Server je weiß welcher Wert kleiner/größer war

→ `statistics.rs:64`: `.lt()` verbraucht den rechten Operanden (Rust-Eigenheit von TFHE-rs) → deshalb vorher klonen

### Median – Batcher Odd-Even Mergesort
→ `statistics.rs:105` (`compare_and_swap`) · `statistics.rs:120` (`batcher_network`) · `statistics.rs:203` (`median`) · `statistics.rs:217–227` (Rayon pro Runde)

**Problem:** Quicksort/Mergesort trifft an verschiedenen Stellen Entscheidungen wie "wenn A > B, gehe links" — das Ergebnis des Vergleichs bestimmt den weiteren Programmfluss. Auf FHE-Daten nicht möglich: der Server darf nie wissen ob true oder false rausgekommen ist.

**Grundidee Sortiernetzwerk:** Man baut das gesamte Muster aller Vergleiche **vorab** — bevor die erste Zahl reinkommt. Egal welche Werte in der Liste stehen, es werden immer exakt dieselben Paare in derselben Reihenfolge verglichen. Kein Branching, keine Entscheidungen zur Laufzeit.

**compare_and_swap** (`statistics.rs:105`): der atomare Baustein
- `.gt()` auf zwei verschlüsselten Werten → liefert einen **verschlüsselten Bool** (der Server weiß nicht ob true/false)
- `if_then_else` berechnet **beide Ausgaben gleichzeitig** (kleiner und größer) — FHE kann nicht nur einen Zweig ausführen
- Ergebnis: das kleinere landet links, das größere rechts — ohne dass der Server die Werte kennt

**Ablauf:**
1. `batcher_network(n)` (`statistics.rs:120`) berechnet einmalig alle Runden als Liste von Index-Paaren — reine Logik, kein FHE
2. Pro Runde: alle Paare sind **disjunkt** (kein Index kommt doppelt vor) → alle `compare_and_swap`-Aufrufe einer Runde laufen **parallel via Rayon** (`statistics.rs:219`)
3. Zwischen Runden: sequentiell — Runde 2 braucht das Ergebnis von Runde 1
4. Nach allen Runden: Liste ist sortiert → Index `(n-1)/2` = Median

**Komplexität:**
- **O(log²n)** sequentielle Runden (z.B. n=8: 6 Runden, n=16: 10 Runden)
- O(n log²n) Vergleiche gesamt
- Innerhalb jeder Runde: alle Paare parallel → Wandzeit = Anzahl Runden × eine FHE-Vergleichsoperation

**Korrektheit:** 0/1-Prinzip (Knuth TAOCP) — ein Sortiernetzwerk das alle binären Eingaben (nur 0 und 1) korrekt sortiert, sortiert auch beliebige Zahlen korrekt. Test: `statistics.rs:251` — alle 2^n Bitmuster bis n=8, kein FHE nötig.

### FheEngine pro Session
→ `fhe.rs:11` (Struct) · `fhe.rs:26` (`start_handler`) · `main.rs:139` (`block_in_place`) · `main.rs:188` (`create_session`)

TFHE-rs braucht den ServerKey auf jedem Rechenthread gesetzt.

Lösung: jede Session bekommt einen eigenen Rayon-Pool, dessen Threads beim Start den Key setzen → parallele Sessions mit verschiedenen Keys stören sich nicht.

Key wird einmalig bei `POST /session` dekomprimiert und gecacht — alle Berechnungsrequests dieser Session nutzen dieselbe Engine.

FHE-Operationen laufen in `block_in_place` → Tokio-Runtime wird nicht blockiert.

**Altes Design (verworfen):** Der `server_key` (~80 MB) wurde mit **jedem einzelnen Request** im Body mitgeschickt — `POST / { server_key, encrypted_list, bit_width }`. Der Server hat ihn dann jedes Mal frisch dekomprimiert. Ergebnis im Stresstest: ~1,2 GB Traffic für 15 Requests, 50% Fehlerrate weil Nginx nach 60 s abbricht. Das neue Session-Design eliminiert diesen Overhead komplett — erster Request lädt den Key hoch, alle weiteren schicken nur die UUID.

### Generics + DivideByElementCount-Trait
→ `statistics.rs:12–26` (Trait + impls) · `main.rs:123` (`compute_statistics_typed`)

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

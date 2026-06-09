# UC5 – Encrypted Statistics Service
## Präsentationsnotizen

---

## Funktionalität · Folie 50

Sechs Statistiken über eine Zahlenliste — **Server sieht nie die echten Werte**:
Summe · Anzahl · Min · Max · Durchschnitt · Median

Der entscheidende Punkt: alles Sicherheitskritische passiert im Browser. Der Server ist eine Rechenmaschine die nicht weiß womit sie rechnet.

**Ablauf (Kurzversion):**
1. Browser generiert Schlüsselpaar (ClientKey bleibt im Browser, verlässt ihn nie)
2. ServerKey einmalig hochladen → Server gibt Session-ID zurück (~80 MB, einmalig)
3. Browser verschlüsselt jeden Wert einzeln
4. Verschlüsselte Liste + Session-ID → Server rechnet homomorph
5. Browser entschlüsselt Ergebnisse lokal mit dem ClientKey

FHE ≠ TLS: Bei TLS sieht der Server die Daten trotzdem. Bei FHE sieht er sie nie — auch nicht kurz, auch nicht intern.

---

## Ablauf: Sequenzdiagramm · Folie 51

Zwei-Phasen-Protokoll:

**Phase 1 — Session anlegen:**
`POST /session { server_key }` — Server dekomprimiert den Key, erstellt FheEngine + Rayon-Thread-Pool, gibt UUID zurück. Das ist der teure Request — ~15–20 s beim ersten Mal.

**Phase 2 — Berechnen:**
`POST /statistics { session_id, encrypted_list, bit_width }` — Server nutzt gecachte Engine, rechnet homomorph, gibt verschlüsselte Ergebnisse zurück.

Key wird nur einmal übertragen. Alle weiteren Requests schicken nur die UUID.

---

## Akteure · Folie 52

**Browser (Client):**
- Einziger Akteur der je die echten Werte kennt
- Generiert Schlüsselpaar komplett lokal — kein Server involviert
- Verschlüsselt jeden Wert einzeln mit dem ClientKey
- Entschlüsselt Ergebnisse lokal — Server sieht das Resultat nie im Klartext
- ClientKey verlässt den Browser nie

**Server:**
- Empfängt ServerKey einmalig, cached ihn pro Session
- Rechnet alle sechs Statistiken homomorph
- Sieht weder Eingabewerte noch Ergebnisse im Klartext
- ServerKey erlaubt nur rechnen, nicht entschlüsseln — auch ein kompromittierter Server kommt nicht an die Daten

---

## Hintergrund: Anforderungen _(kein eigener Slide — Q&A-Vorbereitung)_

- Server darf **nie** Klartext sehen — weder Eingaben noch Ergebnisse
- Alle sechs Kennzahlen korrekt, auch mit negativen Zahlen
- Parallelisierung nötig — FHE ist um Größenordnungen langsamer als normale Arithmetik
- Median braucht Sortierung **ohne Klartext-Vergleiche** — sonst würde der Server aus dem Vergleichsergebnis Informationen über die Werte ableiten können

---

## Lösungen · Folie 53

### Auto-Bitbreite
→ `statistics.component.ts` (`selectOptimalBitWidth`) · `main.rs:251–259` (Dispatch auf bit_width)

Browser wählt die kleinstmögliche Bitbreite anhand von Min/Max der Eingabe:
- alle Werte in [-128, 127] → **Int8**
- alle Werte in [-32.768, 32.767] → **Int16**
- sonst → **Int32**

Kleinere Bitbreite = deutlich schnellere Berechnung (TFHE-Kosten skalieren mit Bitbreite). Faktor 3–8× zwischen Int8 und Int32.

Tradeoff: `bit_width` verrät dem Server die Größenordnung der Werte → bewusst akzeptiert.

### Parallelisierung mit Rayon
→ `statistics.rs:44–66`

Summe, Min, Max und Durchschnitt nutzen alle `par_iter().reduce_with()` aus Rayon. Was das bedeutet:

Statt sequentiell von links nach rechts baut Rayon intern einen **binären Reduce-Baum**: die Liste wird halbiert, beide Hälften werden parallel bearbeitet, Zwischenergebnisse zusammengeführt — rekursiv. Ergebnis: **O(log n)** sequentielle Schritte statt O(n).

Bei Min/Max läuft der Vergleich komplett homomorph:
1. `.lt()` / `.gt()` liefert einen **verschlüsselten Bool** (`FheBool`) — Server weiß nicht ob true oder false
2. `if_then_else` wählt homomorph das Ergebnis aus — Server weiß nie welcher Wert kleiner/größer war

→ `statistics.rs:64`: `.lt()` verbraucht den rechten Operanden (TFHE-rs-Eigenheit) → deshalb vorher klonen

---

## Median – Batcher Odd-Even Mergesort · Folie 54
→ `statistics.rs:105` (`compare_and_swap`) · `statistics.rs:120` (`batcher_network`) · `statistics.rs:203` (`median`) · `statistics.rs:217–227` (Rayon pro Runde)

**Problem:** Quicksort/Mergesort trifft Entscheidungen wie "wenn A > B, gehe links" — das Vergleichsergebnis steuert den Programmfluss. Auf FHE-Daten unmöglich: Server darf nie wissen ob true oder false rausgekommen ist.

**Grundidee Sortiernetzwerk:** Alle Vergleichspaare werden **vorab** festgelegt — bevor die erste Zahl reinkommt. Egal welche Werte in der Liste stehen, es werden immer exakt dieselben Paare in derselben Reihenfolge verglichen. Kein Branching, keine Entscheidungen zur Laufzeit.

**compare_and_swap** (`statistics.rs:105`): der atomare Baustein
- `.gt()` auf zwei verschlüsselten Werten → liefert einen **verschlüsselten Bool**
- `if_then_else` berechnet **beide Ausgaben gleichzeitig** (kleiner und größer) — FHE kann nicht nur einen Zweig ausführen, der Server darf ja nicht wissen welches Ergebnis gilt
- Das kleinere landet links, das größere rechts — ohne dass der Server die Werte kennt

**Ablauf:**
1. `batcher_network(n)` berechnet einmalig alle Runden als Liste von Index-Paaren — reine Logik, kein FHE
2. Pro Runde: alle Paare sind **disjunkt** → alle `compare_and_swap`-Aufrufe einer Runde laufen **parallel via Rayon**
3. Zwischen Runden: sequentiell — Runde 2 braucht das Ergebnis von Runde 1
4. Nach allen Runden: sortiert → Index `(n-1)/2` = Median

**Komplexität:** O(log²n) sequentielle Runden, innerhalb jeder Runde alle Paare parallel

**Korrektheit:** 0/1-Prinzip (Knuth TAOCP) — Netzwerk das alle binären Eingaben korrekt sortiert, sortiert auch beliebige Zahlen korrekt. Test: `statistics.rs:251` — alle 2^n Bitmuster bis n=8, kein FHE nötig.

---

## FheEngine & Session · Folie 55
→ `fhe.rs:11` (Struct) · `fhe.rs:26` (`start_handler`) · `main.rs:139` (`block_in_place`) · `main.rs:188` (`create_session`)

**Problem: ServerKey ist thread-lokal**
TFHE-rs setzt den ServerKey per Rayon-Thread. Mit globalem Pool: zwei parallele Sessions überschreiben sich gegenseitig den Key — stiller Fehler, falsche Ergebnisse ohne Crash.

**Lösung: dedizierter Rayon-Pool pro Session**
- `POST /session` erstellt eigene `FheEngine` mit eigenem Rayon-Pool
- `start_handler` setzt den Key auf jedem Thread beim Start
- Parallele Sessions sind vollständig isoliert
- Key einmalig dekomprimiert und gecacht — alle weiteren Requests nutzen dieselbe Engine

**FHE blockiert den Thread**
FHE-Ops dauern 2–20 Sekunden am Stück. Tokio-Runtime-Thread darf nicht blockiert werden — sonst können keine weiteren Requests angenommen werden. `block_in_place` signalisiert Tokio "ich blockiere jetzt bewusst".

**Altes Design (verworfen):** `POST / { server_key, encrypted_list, bit_width }` — Key mit jedem Request mitschicken. Ergebnis: ~1,2 GB Traffic für 15 Requests, 50% Fehlerrate (Nginx-Timeout nach 60 s).

---

## Generics + DivideByElementCount-Trait _(kein eigener Slide — Q&A-Vorbereitung)_
→ `statistics.rs:12–26` (Trait + impls) · `main.rs:123` (`compute_statistics_typed`)

Drei Bitbreiten (Int8/16/32), aber dieselbe Logik für alle.

Problem: TFHE-rs hat keine generische Division — `FheInt8 / i8`, `FheInt16 / i16` usw. sind getrennt, kein gemeinsamer Trait.

Lösung: eigenes `DivideByElementCount`-Trait → eine generische Funktion statt dreimal Copy-Paste.

---

## Was weiß der Server? & Performance · Folie 56

| | sichtbar? |
|---|---|
| Anzahl der Werte (n) | ja |
| Größenordnung der Werte | ja (über `bit_width`) |
| Die konkreten Werte | **nein** |
| Summe, Min, Max, Avg, Median | **nein** |
| ClientKey | **nein** — verlässt den Browser nie |
| ServerKey | ja — einmalig hochgeladen, erlaubt nur rechnen, nicht entschlüsseln |

**Performance (Netcup-Server, 1 paralleler Nutzer, gemessen mit k6):**

| Konfiguration | 95% der Requests unter |
|---|---|
| n=5, Int8 | 2,23 s |
| n=10, Int8 | 4,46 s |
| n=10, Int16 | 7,95 s |
| n=10, Int32 | 16,16 s |

Ohne Session-Caching: +~18 s pro Request (Key-Upload-Overhead).

**Stresstest** (bis 10 parallele Nutzer): 52% Fehlerrate ab ~2–3 gleichzeitigen Requests → CPU-Engpass, Nginx-Timeout nach 60 s.

---

## Einschränkungen · Folie 57

- **Kein Float** — TFHE-rs hat keine Float-Typen; Durchschnitt ist ganzzahlig (1,5 → 1); Bibliotheks-Einschränkung, kein Design-Fehler
- **bit_width-Leak** — verrät Größenordnung der Werte; bewusst akzeptiert wegen Performance-Gewinn
- **Session-Lebensdauer** — 10 min Idle-Timeout; danach 404, Key muss neu hochgeladen werden; konfigurierbar
- **Listengröße** — n > ~20 mit Int32 dauert mehrere Minuten; Service ist für kleine Listen konzipiert

---

## Herausforderungen · Folie 58

**Stateless-Design hat versagt**
Der ServerKey ist ~80 MB komprimiert, ~1,2 GB unkomprimiert. Erster Entwurf hat ihn mit jedem einzelnen Berechnungsrequest mitgeschickt. Im Stresstest (15 parallele Requests): Server crasht, Nginx-Timeout nach 60 s, 50% Fehlerrate. Lösung: Session-basiertes Design, Key einmalig hochladen.

**TFHE-rs `.lt()` konsumiert den Operanden**
`.lt(other)` nimmt `other` by value — der Wert ist danach weg. Wenn man dann `if_then_else(&a, &b)` aufrufen will, ist `b` bereits moved. Compilerfehler, nicht sofort offensichtlich. Lösung: vorher klonen. → `statistics.rs:65`

**Keine generische Division in TFHE-rs**
`FheInt8 / i8`, `FheInt16 / i16`, `FheInt32 / i32` sind getrennt, kein gemeinsamer Trait. `compute_statistics_typed` generisch zu halten erforderte ein eigenes `DivideByElementCount`-Trait. → `statistics.rs:12`

**Tokio + blockende FHE-Operationen**
FHE-Ops dauern 2–20 Sekunden am Stück. In async Tokio blockiert das den Runtime-Thread — keine weiteren Requests können angenommen werden. Erst unter Last aufgefallen, nicht beim Entwickeln. Lösung: `block_in_place`. → `main.rs:139`

**ServerKey Thread-Local in TFHE-rs**
TFHE-rs setzt den ServerKey thread-lokal per Rayon-Thread. Globaler Pool: parallele Sessions mit unterschiedlichen Keys könnten sich gegenseitig überschreiben. Lösung: dedizierter Pool pro Session mit `start_handler`. → `fhe.rs:26`

**Batcher-Korrektheit ohne FHE testen**
Sortiernetzwerk verifizieren ohne FHE auszuführen (dauert Minuten)? 0/1-Prinzip (Knuth TAOCP): Netzwerk das alle binären Eingaben korrekt sortiert, sortiert auch beliebige Zahlen korrekt. Test läuft in Millisekunden, kein FHE nötig. → `statistics.rs:251`

---

## Architekturentscheidungen _(kein eigener Slide — Q&A-Vorbereitung)_

| Entscheidung | Verworfen | Grund |
|---|---|---|
| Session-basiert (`POST /session` → UUID) | Key pro Request im Body | ~1,2 GB pro 15 Requests → Server crashte, 50% Fehlerrate |
| Batcher Odd-Even Mergesort | Quicksort, Mergesort | FHE: Vergleichsergebnis darf nie im Klartext vorliegen |
| Dedizierter Rayon-Pool pro Session | Globaler Rayon-Pool | ServerKey thread-lokal in TFHE-rs — globaler Pool überschreibt Keys |
| Auto-Bitbreite (Int8/16/32) | Immer Int32 | Faktor 3–8× schneller; Int32 wäre für kleine Werte massiv überdimensioniert |
| `DivideByElementCount`-Trait | 3× Copy-Paste | TFHE-rs hat keine generische Division |

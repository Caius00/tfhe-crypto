# UC5 – Encrypted Statistics Service
## Präsentationsvorbereitung

---

## Was macht UC5?

Der Service nimmt eine Liste von Zahlen entgegen und berechnet darauf sechs statistische Kennzahlen – ohne dass der Server jemals die echten Werte sieht. Alles läuft verschlüsselt, die Ergebnisse kommen auch verschlüsselt zurück und werden erst beim Client entschlüsselt.

Die sechs Kennzahlen:
- Summe
- Anzahl
- Minimum
- Maximum
- Durchschnitt
- Median

Der Ablauf auf hoher Ebene:
1. Client generiert ein Schlüsselpaar (ClientKey + ServerKey)
2. Client verschlüsselt jeden Wert einzeln und schickt die Liste + ServerKey an den Server
3. Server rechnet homomorph auf den verschlüsselten Daten
4. Server schickt verschlüsselte Ergebnisse zurück
5. Client entschlüsselt lokal mit dem ClientKey

Der ServerKey erlaubt dem Server nur zu rechnen, nicht zu entschlüsseln. Entschlüsseln geht ausschließlich mit dem ClientKey, der den Browser nie verlässt.

---

## Anforderungen

- Der Server darf die Eingabewerte und Ergebnisse **nie im Klartext sehen** – das ist die Kernbedingung
- Alle sechs Kennzahlen müssen korrekt berechnet werden, auch mit negativen Zahlen
- Die Berechnungen müssen parallelisiert werden – FHE-Operationen sind um ein Vielfaches teurer als normale Rechenoperationen, bei großen Listen wäre ein rein sequentieller Ansatz inakzeptabel
- Für den Median brauchen wir einen Sortieralgorithmus der **komplett ohne Klartext-Vergleiche** auskommt, weil der Server bei einem normalen Sortieralgorithmus entscheiden müsste welcher Wert größer ist – das würde Informationen über die verschlüsselten Werte leaken

---

## Wie haben wir das gelöst?

### Grundprinzip – FHE mit TFHE-rs

Wir nutzen vorzeichenbehaftete FHE-Integer aus der TFHE-rs Bibliothek: `FheInt8`, `FheInt16` und `FheInt32`. Der Client verschlüsselt jeden Wert einzeln (jeder Ciphertext ist dabei unabhängig von den anderen), schickt die Liste mit dem ServerKey an den Server, der Server rechnet homomorph und gibt verschlüsselte Ergebnisse zurück. Der Client entschlüsselt dann lokal.

Wichtig: Summe und Durchschnitt geben wir eine Bitbreite breiter zurück als die Eingabe – also bei Int8-Eingabe ein Int16 als Summe. Das ist nötig weil z.B. 100 × 127 (n Werte × Max-Wert bei Int8) nicht mehr in ein Int8 passt. Der Server merkt das nicht, er rechnet einfach auf dem breiteren Typ.

### Auto-Bitbreite – warum das wichtig ist

Je kleiner die Bitbreite, desto schneller die Berechnung. TFHE-rs-Kosten skalieren direkt mit der Bitbreite. Der Client schaut sich Min und Max der Eingabe an und wählt die kleinstmögliche Breite:

- Werte passen in [-128, 127] → **Int8**
- Werte passen in [-32.768, 32.767] → **Int16**
- Sonst → **Int32**

Bei unserer Messung mit 6 Werten und Int8 lagen wir bei ~18,9 Sekunden Gesamtlatenz. Mit Int32 wäre dieselbe Liste deutlich langsamer gewesen. Die Optimierung passiert vollständig auf Client-Seite, der Server bekommt nur die fertige `bit_width` und die verschlüsselten Werte.

Der Nachteil: `bit_width` verrät dem Server die Größenordnung der Werte. Int8 heißt, alle Werte liegen in [-128, 127] – das ist ein kleines Privacy-Leak. Für diesen Use Case haben wir das als akzeptabel eingestuft, weil der Gewinn bei der Performance deutlich überwiegt.

### Parallelisierung mit Rayon

Summe, Min, Max und Durchschnitt nutzen alle `par_iter()` mit `reduce_with()` aus der Rayon-Bibliothek. Das erzeugt intern einen Reduce-Baum: die Liste wird halbiert, beide Hälften parallel reduziert, dann das Ergebnis zusammengeführt – O(log n) Tiefe statt O(n) sequentiell.

Bei Min und Max läuft der Vergleich komplett homomorph:
- `.lt()` bzw. `.gt()` gibt einen verschlüsselten Bool (`FheBool`) zurück
- `if_then_else` wählt dann homomorph das Ergebnis aus – der Server weiß nie welcher der beiden Werte tatsächlich kleiner oder größer war

Eine Besonderheit bei TFHE-rs: `.lt()` nimmt den rechten Operanden by value, also konsumiert ihn. Deshalb müssen wir den Wert vorher klonen, damit er danach noch für `if_then_else` verfügbar ist. Das ist eine Eigenheit der Bibliothek, kein Design-Fehler.

### Median – Batcher Odd-Even Mergesort

Das war die technisch aufwändigste Komponente. Ein normaler Sortieralgorithmus wie Quicksort oder Mergesort macht an verschiedenen Stellen Entscheidungen basierend auf Vergleichsergebnissen – also zum Beispiel "wenn A > B, gehe in den linken Zweig". Auf FHE-Daten geht das nicht, weil der Server nie das Ergebnis eines Vergleichs im Klartext sehen darf.

Die Lösung: **Batcher's Odd-Even Mergesort Netzwerk**

Die Idee dahinter ist, dass man ein Sortiernetzwerk aus Komparatoren aufbauen kann, bei dem die Reihenfolge aller Vergleiche komplett **vor der Ausführung** feststeht – unabhängig von den Werten. Das Netzwerk macht immer genau dieselben Vergleiche, egal welche Zahlen drin stehen. Das ist ideal für FHE.

Das Netzwerk besteht aus Runden. Innerhalb einer Runde sind alle Vergleichspaare disjunkt – kein Index kommt doppelt vor. Das bedeutet, wir können alle Paare einer Runde parallel ausführen. Zwischen Runden gibt es eine Abhängigkeit, die laufen sequentiell.

Komplexität: O(log²n) sequentielle Runden, innerhalb jeder Runde alle Paare parallel mit Rayon.

Nach dem Durchlauf ist die Liste sortiert und wir nehmen einfach Index `(n-1)/2` als Median heraus – bei gerader Länge den Lower Median.

Die Korrektheit testen wir über das **0/1-Prinzip** aus Knuth TAOCP: Ein Comparator-Netzwerk das alle binären Eingaben (nur 0en und 1en) korrekt sortiert, sortiert auch beliebige Eingaben korrekt. Wir testen alle 2^n Bitmuster bis n=8 – kein FHE nötig für diesen Test, reine Logik.

### FheEngine – warum ein eigener Thread-Pool

TFHE-rs hat eine Besonderheit: der ServerKey muss auf jedem Thread der FHE-Operationen ausführt gesetzt sein, bevor er rechnen kann. Bei einem normalen globalen Rayon-Pool wäre das ein Problem – verschiedene gleichzeitige Requests könnten sich gegenseitig den ServerKey überschreiben.

Deshalb erstellen wir pro Request eine eigene `FheEngine` mit einem dedizierten Rayon-Thread-Pool. Beim Start jedes Worker-Threads läuft ein `start_handler` der den ServerKey auf diesem Thread setzt. So sind parallele Requests vollständig voneinander isoliert.

Dazu kommt: FHE-Berechnungen sind CPU-intensiv und blockieren den Thread für Sekunden. In einem async Rust-Service (Tokio) darf man den Runtime-Thread nicht blockieren, sonst können keine anderen Requests mehr angenommen werden. Deshalb rufen wir alle FHE-Operationen innerhalb von `block_in_place` auf – das signalisiert Tokio "ich blockiere jetzt, stell einen anderen Thread bereit".

### Generics + eigenes Trait – warum der Aufwand?

Das ist etwas das auf den ersten Blick nach Over-Engineering aussieht, aber einen konkreten Grund hat.

Wir unterstützen drei Bitbreiten: Int8, Int16 und Int32. Alle Statistikfunktionen (Summe, Min, Max, Median) funktionieren für alle drei Typen exakt gleich – der einzige Unterschied ist der konkrete FHE-Typ. Die naheliegende Lösung wäre copy-paste für jede Bitbreite, also drei mal dieselbe Funktion. Das haben wir mit Generics vermieden.

Das Problem dabei: TFHE-rs hat keine generische Division. `FheInt16 / i16`, `FheInt32 / i32` und `FheInt64 / i64` sind separat implementiert und lassen sich nicht über einen gemeinsamen Typ vereinheitlichen. Rust erlaubt kein `FheIntX / i32` generisch.

Die Lösung: wir haben ein eigenes `DivideByElementCount`-Trait gebaut. Jeder FHE-Typ bekommt eine kurze `impl` die den Cast auf den passenden Skalartyp macht und dann dividiert. Das Trait kapselt diese Eigenheit von TFHE-rs nach außen weg, und die generische `compute_statistics_typed`-Funktion kann alle drei Bitbreiten über denselben Code abdecken.

Vorteil: Eine Funktion statt drei. Wenn wir z.B. Int64-Unterstützung hinzufügen wollten, reichen ein neues `impl DivideByElementCount for FheInt64` und ein neuer `match`-Arm im Handler.

---

## Was weiß der Server, was nicht?

| | sichtbar für den Server? |
|---|---|
| Anzahl der Eingabewerte | ja – steht direkt in der Array-Länge |
| Größenordnung der Werte | ja – über `bit_width` (Int8 heißt alle Werte in [-128, 127]) |
| Die konkreten Werte | nein |
| Summe, Min, Max, Avg, Median | nein |
| ClientKey | nein – verlässt den Browser nie |
| ServerKey | ja – wird im Request mitgeschickt, erlaubt aber nur rechnen, nicht entschlüsseln |

---

## Bekannte Einschränkungen

**Kein Float-Support**
TFHE-rs hat keine Float-Typen. Der Durchschnitt ist ganzzahlig – 1,5 wird zu 1. Das ist in der Spec dokumentiert.

**Privacy-Leak über bit_width**
`bit_width=8` verrät dem Server dass alle Werte in [-128, 127] liegen. Bei kleinen Listen mit bekanntem Kontext könnte man daraus Werte eingrenzen. Für diesen UC als akzeptabel eingestuft.

**Kein Session-Caching**
Der ServerKey wird bei jedem Request neu dekomprimiert – das kostet jedes Mal mehrere Sekunden. UC3 und UC8 lösen das anders: dort wird der Key einmal hochgeladen und serverseitig in einer Session gehalten. Für UC5 haben wir den einfacheren stateless Ansatz gewählt.

**Performance-Grenze**
Bei n > ~20 Werten mit Int32 übersteigt die Rechenzeit mehrere Minuten. Der Service ist für kleine Listen konzipiert. Das Batcher-Netzwerk sortiert außerdem immer die gesamte Liste, statt nur den Median-Index zu isolieren – ein spezialisiertes Partial-Sort-Netzwerk wäre schneller, war aber für den Umfang dieses Projekts nicht gerechtfertigt.

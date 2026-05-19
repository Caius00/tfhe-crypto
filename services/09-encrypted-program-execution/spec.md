# Spezifikation
**für 09-encrypted-program-execution**
> [!NOTE]
> Pro umgesetztem Use Case sind die folgenden acht Sektionen verpflichtend. Die Sektionsstruktur ist für alle UCs identisch; die Detailtiefe darf je nach UC-Komplexität variieren (UC2 wird hier zwangsläufig weniger Inhalt haben als UC9).
---

### Funktionsbeschreibung
> [!NOTE]
> Welches Problem löst der UC, wer sind die Akteure (typischerweise Initiator E vs. weitere Clients vs. Server), wie sieht der Lebenszyklus einer Session aus (Erstellung, Teilnahme, Auswertung, Schließen). Ein Verhaltensdiagramm gehört hierher. 

### OpenAPI-Schnittstelle
> [!NOTE]
> Vollständige Definition aller Endpunkte des UC: Request- und Response-Schemata, Auth-Header (falls vorhanden), Status-Codes, Fehlerantworten. Die Sektion muss zur tatsächlichen Implementierung passen und idealerweise aus dem Code generiert sein. Wenn ein Endpunkt einen verschlüsselten Body erwartet, beschreibt das Schema, was dort erwartet wird (z.B. „bincode-serialisierter FheUint8"), nicht nur „opaque blob".

### Trust- und Threat-Model
> [!NOTE]
> Tabelle mit den Spalten *Datum* | *am Server klar* | *am Server verschlüsselt* | *nur am Client*. Jedes relevante Datum (Eingabewerte, Optionsnamen, Voter-IDs, Session-IDs, Grenzwerte, Zeitstempel, …) bekommt eine Zeile. Darunter eine Analyse beobachtbarer (Meta)daten und was ein bösartiger Server-Operator damit anfangen könnte (Anzahl Stimmen, Sequenzlängen, Request-Timing, Aufruffrequenz). 
> Außerdem: Restvertrauen in den Server-Operator (was hängt nicht an FHE, sondern an korrektem Server-Verhalten?), Annahmen außerhalb von FHE (TLS, Frontend, TFHE-rs als Black Box, fehlende Auth am Gateway) und ein konkreter Satz, was am Schutzversprechen nicht produktreif ist und was genau eigentlich versprochen wird („Der Server kennt deine DNA nicht“, …) Diese Sektion ist die wichtigste der ganzen Spec – hier wird FHE konkret.

### FHE-Designentscheidungen
> [!NOTE]
> Welche TFHE-rs-Typen verwendet ihr (FheBool, FheUint8, FheInt32, FheUint64, …) und warum diese Bitbreite? Welche Operationen werden gebraucht (Add, Mul, Vergleich, Bit-Shift) und welche davon sind auf dem gewählten Typ teuer?
> Welche fachlich naheliegende Variante habt ihr verworfen und mit welcher Begründung (z.B. „Float wäre semantisch passend, aber TFHE-rs bietet keine sinnvolle Division")? Falls Approximationen verwendet werden: mit welchem Fehlerprofil?

### Komplexität der eigenen Algorithmen
> [!NOTE]
> Zeit- und Platzkomplexität der **eigenen** Algorithmen, in Abhängigkeit der relevanten Parameter (Sequenzlänge n, Stimmenanzahl k, Bitbreite b, Leaderboard-Länge m, …). 
> Wo möglich kurz hergeleitet oder bewiesen, nicht nur behauptet. Die interne Komplexität von TFHE-rs (Bootstrapping, Polynom-Multiplikation, …) ist **nicht** Gegenstand dieser Sektion – die Library wird als gegeben behandelt. Eine einzelne homomorphe Operation gilt für die Komplexitätsanalyse als O(1)-Baustein

### Performance-Messung
> [!NOTE]
> Latenzen (p50 und p95) auf dem Netcup-Server unter konkret beschriebenen Bedingungen (Anzahl gleichzeitiger Clients, Eingabegröße), (optional: RAM-Bedarf pro aktiver Session), sowie eine Aussage zur **Throughput-Grenze**: ab welcher Anzahl gleichzeitiger Requests pro Sekunde steigt p95 deutlich an, treten Timeouts auf oder läuft der RAM voll?
> Mess-Setup reproduzierbar dokumentiert (Skript, Eingabewerte, Datum, verwendete TFHE-rs-Parameter). Mindestens eine Lastkurve über p95, die zeigt, wo das System unter realistischen Bedingungen kippt. Die Zahlen müssen aus echten Messungen kommen, nicht aus Schätzungen.
> Endpunkte, die nur eine Art „Grundrauschen“ darstellen und nicht maßgeblich für die Performance des Use Cases verantwortlich sind, können begründet ignoriert werden.
> Bestenfalls wird aber ein Happy-Flow über mehrere sequentielle Requests definiert und in Summe gemessen.

### Limitationen
> [!NOTE]
> Was funktioniert *nicht* oder nur eingeschränkt. Was wurde bewusst nicht umgesetzt und warum (z.B. „kein ZKP gegen Doppelabstimmung – stattdessen Voter-Zulassung über den Initiator"). Welche Eingabegrößen lässt das System nicht mehr zu? Welche Operationen wären achlich erwünscht, aber technisch nicht machbar (Division, Float-Arithmetik, datenabhängige Branches)?
> Diese Sektion soll nicht defensiv klingen – Limitationen sind so relevant wie Fähigkeiten.

---
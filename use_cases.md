# Use-Case Anforderungen:

### 1: Encrypted Key-Value Store
*3 Punkte*
- Client verschlüsselt Keys und Values und sendet sie zum persistieren an den Server
- Operationen: Put, Get by Key, Exists, Clear
	- Der Server weiß weder, was drinnen ist, noch was abgefragt wird oder existiert
- Time to live (Klartext Metadatum) einzelner Einträge am Server konfigurierbar machen
- Achtung: KV-Stores sollten pro Client isoliert sein
- KV-Store nicht in-memory sondern in Redis
---

### 2: Encrypted Age Verification
*3 Punkte*
- Der Client sendet einen numerischen Wert an 
- Der Server vergleicht "Wert > Grenzwert" und sendet das Ergebnis zurück an den Client
- Beachte: der Grenzwert soll im Server als Klartext hinterlegt sein und es muss eine Klartext-Cyphertext-Operation durchgeführt werden
---

### 3: Encrypted Voting / Polling
*8 Punkte*
- Ein Client E erstellt eine Voting Session
	- Dabei ist egal, ob die Optionsnamen verschlüsselt sind oder nicht
- Andere Clients senden ihre Stimme als Ganzzahl
	- ID der Session darf im Klartext vorliegen
- Der Server aggregiert die Stimmen pro Aspekt
- Zu einem beliebigen Zeitpunkt kann E die aggregierten Ergebnisse vom Server abrufen
- Abstimmende Clients müssen bei E anfragen und zugelassen werden
	- Abstimmende Clients müssen ihren verschlüsselt ihren Namen mitschicken
---

### 4: Sealed-Bid Auction
*8 Punkte*
- Ein Client E erstellt eine Auktion
- Mehrere Client geben ihre Gebote in Euro (ganzzahlig) unter Angabe ihres verschlüsselten Namens ab
- Zu einem beliebigen Zeitpunkt kann E die Auktion "auswerten" und den Gewinner auswerten lassen
	- Dafür wertet der Server aus, welcher Client (Stichwort Identifizierung) den höchsten Betrag geboten hat und wie hoch der Betrag ist
---

### 5: Encrypted Statistics Service
*8 Punkte*
- Ein Client lädt eine Liste von Ganzzahlen hoch
- Der Server bestimmt:
	- Summe, Anzahl, Min, Max, Durschnitt (abgerundet)
- Der Server bestimmt ebenfalls den Median
---

### 6: Encrypted Genomics
*8 Punkte*
- Ein Client E erstellt eine Session und lädt seine verschlüsselte Sequenz hoch
- Weitere Client können ihre Sequenzen dann und Angabe des verschlüsselten Namens vergleichen
	- Es gibt einen Grenzwert, der "ähnlich genug" bedeutet
	- E kann anschließend abrufen, wer ihm ähnlich ist und wer nicht
- Risk-Marker Check:
	- Ein bekanntes (unverschlüsseltes) Risikomuster (z.b. 30020311123010) wird per Sliding-Window homomorph gegen die verschlüsselte Sequenz geprüft
- Fachliche Interpretationen dürfen gerne dargestellt werden
- Technische Details:
	- Vergleich (mindestens) als Hamming Distanz implementieren
	- Sequenz als Vec<FheInt> oder ähnlich darstellen
	- Wir nehmen (mindestens) an, dass alle Sequenzen die gleiche Länge haben
---

### 7: Encrypted Image Processing
*13 Punkte*
- Der Client verschlüsselt Pixelwerte eines Graustufenbilds (Integer-Array)
- Der Server führt homomorphe Bildoperationen aus:
	- Weißschwelle: Pixel > 128 werden zu 255 geändert
	- Invertierung: Pixel = 255 - Pixel
- Der Client zeigt das Bild dann wieder an
---

### 8: Encrypted Leaderboard
*15 Punkte*
- Ein Client E erstellt ein Leaderboard
- Andere Clients können ihre verschlüsselten Scores an diese Session senden
	- Der Server fügt die neuen Werte ein und sortiert die Liste entsprechend sofort
	- Clients senden eine verschlüsselte Kennung mit ihrem Score mit
	- Optional: Es gibt Flappy Bird und der Score wird automatisch übertragen
- Nur E kann das Leaderboard einsehen
- E kann abfragen, welchen Rang oder welche Ränge eine bestimmte Kennung hat (je nachdem, ob überschrieben wird)
---

### 9: Encrypted Program Execution
*21 Punkte*
- Auf dem Server läuft eine CPU-Simulation, welche Arithmetik, Sprünge und bedingte Sprünge unterstützt
- Der Client kann ein Programm mit entsprechendem Instructionset schreiben, verschlüsseln und vom Server ausführen lassn
- Der Server kennt zu keiner Zeit das Programm oder die Daten 
---

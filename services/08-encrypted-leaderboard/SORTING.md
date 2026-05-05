# Sorting

Das Leaderboard verwendet **Batcher's Odd-Even Mergesort** — einen Algorithmus der eine feste, vorher bekannte Folge von Vergleichen durchführt, unabhängig davon wie die Daten aussehen. Das ist notwendig weil FHE-Vergleiche keinen echten Branch erlauben: das Ergebnis ist immer verschlüsselt, der Server weiß nie ob `true` oder `false`.

Der Algorithmus teilt die Liste in zwei Hälften, sortiert beide rekursiv, und merged sie anschließend. Beim Merge werden zuerst alle Elemente an geraden Positionen zusammengeführt, dann alle an ungeraden — daher "Odd-Even". Dadurch entstehen Vergleichspaare die sich nie überschneiden und mit Rayon parallel auf mehreren CPU-Cores laufen.

Das Ergebnis: bei 20 Einträgen ~130 Vergleiche statt 361 (Bubble Sort), verteilt auf ~15 parallele Runden à ~5 Sekunden = **~75 Sekunden** statt ~30 Minuten.

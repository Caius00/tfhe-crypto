import { Component, Input } from '@angular/core';
import { CommonModule } from '@angular/common';
import { CardComponent } from '../../../../shared/components/card/card.component';
import { EmptyStateComponent } from '../../../../shared/components/empty-state/empty-state.component';
import { Question } from '../../voting.types';

/**
 * Ein einzelnes entschlüsseltes Ergebnis pro Frage.
 *  - string:    skalares Ergebnis (z.B. Numeric: "42", Bool: "Ja: 5")
 *  - string[]:  pro-Option Ergebnis (z.B. Single/Multiple Choice: ["Apfel: 3", "Birne: 1"])
 */
export type DecryptedResult = string | string[];

/**
 * Reine Anzeige der entschlüsselten Voting-Ergebnisse.
 * Erwartet Frage- und Ergebnis-Listen mit gleichem Index.
 */
@Component({
  selector: 'app-results-view',
  standalone: true,
  imports: [CommonModule, CardComponent, EmptyStateComponent],
  templateUrl: './results-view.component.html',
  styleUrl: './results-view.component.css',
})
export class ResultsViewComponent {
  /** Fragen der Session (für Anzeige des Fragetexts pro Ergebnis) */
  @Input() questions: Question[] = [];
  /** Entschlüsselte Ergebnisse, gleicher Index wie `questions` */
  @Input() results: DecryptedResult[] = [];

  /** Type-guard für Template: ist das Ergebnis ein Array (Multi-Option)? */
  isArray(r: DecryptedResult): r is string[] {
    return Array.isArray(r);
  }
}

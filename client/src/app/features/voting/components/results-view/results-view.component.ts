import { Component, computed, Input, Signal, signal } from '@angular/core';
import { CommonModule } from '@angular/common';
import { CardComponent } from '../../../../shared/components/card/card.component';
import { EmptyStateComponent } from '../../../../shared/components/empty-state/empty-state.component';
import { Question } from '../../voting.types';
import { ButtonComponent } from '../../../../shared/components/button/button.component';

/**
 * Ein einzelnes entschlüsseltes Ergebnis pro Frage.
 *  - string:    skalares Ergebnis (z.B. Numeric: "42")
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
  imports: [CommonModule, CardComponent, EmptyStateComponent, ButtonComponent],
  templateUrl: './results-view.component.html',
  styleUrl: './results-view.component.css',
})
export class ResultsViewComponent {
  /** Fragen der Session (für Anzeige des Fragetexts pro Ergebnis) */
  @Input() questions: Question[] = [];
  /** Entschlüsselte Ergebnisse, gleicher Index wie `questions` */
  @Input() results: DecryptedResult[] = [];
  @Input() isDecrypted = false;

  /** Type-guard für Template: ist das Ergebnis ein Array (Multi-Option)? */
  isArray(r: DecryptedResult): r is string[] {
    return Array.isArray(r);
  }

  expandedResults = signal<number[]>([]);

  isExpanded(index: number): boolean {
    return this.expandedResults().includes(index);
  }

  toggleResult(index: number): void {
    if (this.isExpanded(index)) {
      this.expandedResults.set(
        this.expandedResults().filter(i => i !== index)
      );
    } else {
      this.expandedResults.set([
        ...this.expandedResults(),
        index
      ]);
    }
  }


}

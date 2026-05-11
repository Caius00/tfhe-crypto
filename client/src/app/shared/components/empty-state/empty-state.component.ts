import { Component, Input } from '@angular/core';

/**
 * Platzhalter wenn keine Daten vorhanden sind.
 *
 * Beispiel:
 *   <app-empty-state
 *     icon="📭"
 *     title="Keine Anfragen"
 *     message="Es wartet niemand auf Freigabe." />
 */
@Component({
  selector: 'app-empty-state',
  standalone: true,
  templateUrl: './empty-state.component.html',
  styleUrl: './empty-state.component.css',
})
export class EmptyStateComponent {
  /** Optionales Icon (Emoji oder Unicode-Symbol) */
  @Input() icon = '';
  /** Hauptüberschrift */
  @Input() title = '';
  /** Erläuternder Untertext */
  @Input() message = '';
}

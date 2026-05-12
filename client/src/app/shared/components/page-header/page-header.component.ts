import { Component, Input } from '@angular/core';

/**
 * Konsistenter Seiten-Header für alle Feature-Seiten.
 *
 * Zeigt einen Titel + optional Untertitel + optional einen Slot für Aktionen
 * (z.B. Buttons rechts).
 *
 * Beispiel:
 *   <app-page-header title="Voting" subtitle="Verschlüsselte Umfragen">
 *     <app-button label="Neu" (clicked)="create()" />
 *   </app-page-header>
 */
@Component({
  selector: 'app-page-header',
  standalone: true,
  templateUrl: './page-header.component.html',
  styleUrl: './page-header.component.css',
})
export class PageHeaderComponent {
  @Input() title = '';
  @Input() subtitle = '';
}

import { Component, EventEmitter, Input, Output } from '@angular/core';
import { CommonModule } from '@angular/common';
import { ButtonComponent } from '../../../../shared/components/button/button.component';
import { EmptyStateComponent } from '../../../../shared/components/empty-state/empty-state.component';

/**
 * Eintrag in der Pending-Liste.
 *  participantId:  ID des wartenden Teilnehmers
 *  encNameChunks:  verschlüsselter Name (Base64-Chunks pro Zeichen)
 *  decryptedName:  optionaler entschlüsselter Name (bereits dekryptiert)
 */
export interface PendingEntryView {
  participantId: string;
  encNameChunks: string[];
  decryptedName?: string;
}

/**
 * Liste aller Teilnehmer die auf Freigabe warten.
 *
 * Stellt Aktionen bereit: Name entschlüsseln, Annehmen, Ablehnen.
 * Komplett präsentational – Logik liegt im Parent.
 */
@Component({
  selector: 'app-pending-list',
  standalone: true,
  imports: [CommonModule, ButtonComponent, EmptyStateComponent],
  templateUrl: './pending-list.component.html',
  styleUrl: './pending-list.component.css',
})
export class PendingListComponent {
  /** Anzuzeigende Teilnehmer */
  @Input() entries: PendingEntryView[] = [];
  /** Wird gerade gerade etwas geladen? (deaktiviert Buttons) */
  @Input() busy = false;

  /** Klick auf "Name entschlüsseln" */
  @Output() decrypt = new EventEmitter<PendingEntryView>();
  /** Klick auf "Annehmen" */
  @Output() approve = new EventEmitter<string>();
  /** Klick auf "Ablehnen" */
  @Output() reject = new EventEmitter<string>();
}

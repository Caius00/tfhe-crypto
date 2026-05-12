import { Component, Input, Output, EventEmitter } from '@angular/core';
import { NgClass } from '@angular/common';
import { SpinnerComponent } from '../spinner/spinner.component';

/**
 * Wiederverwendbarer Button mit Varianten, Größen und Loading-State.
 *
 * Beispiele:
 *   <app-button label="Speichern" (clicked)="save()" />
 *   <app-button label="Senden" variant="primary" [loading]="isSending()" />
 *   <app-button label="Löschen" variant="danger" size="sm" />
 *   <app-button label="Ablehnen" variant="secondary" [disabled]="!ready" />
 */
@Component({
  selector: 'app-button',
  imports: [NgClass, SpinnerComponent],
  templateUrl: './button.component.html',
  styleUrl: './button.component.css',
})
export class ButtonComponent {
  /** Beschriftung des Buttons */
  @Input() label = 'Submit';
  /** HTML-type (für Forms: 'submit') */
  @Input() type: 'button' | 'submit' | 'reset' = 'button';
  /** Deaktiviert den Button (kein Klick möglich) */
  @Input() disabled = false;
  /** Visuelles Stil-Schema */
  @Input() variant: 'primary' | 'secondary' | 'danger' | 'ghost' = 'primary';
  /** Größe */
  @Input() size: 'sm' | 'md' | 'lg' = 'md';
  /** Wenn true: Spinner statt Label, Klicks blockiert */
  @Input() loading = false;
  /** Streckt den Button auf 100% Breite */
  @Input() fullWidth = false;
  /** Optionales Icon (Unicode/Emoji) vor dem Label */
  @Input() icon: string | null = null;

  @Output() clicked = new EventEmitter<void>();

  // Klick wird blockiert wenn loading aktiv ist – verhindert Doppel-Submits
  onClick(): void {
    if (this.loading || this.disabled) return;
    this.clicked.emit();
  }
}

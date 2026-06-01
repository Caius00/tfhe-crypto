import { Component, Input, Output, EventEmitter } from '@angular/core';

/**
 * Wiederverwendbare Checkbox mit Label.
 *
 * Beispiel:
 *   <app-checkbox
 *     label="Mehrfachauswahl erlauben"
 *     [checked]="multi()"
 *     (checkedChange)="multi.set($event)" />
 */
@Component({
  selector: 'app-checkbox',
  standalone: true,
  templateUrl: './checkbox.component.html',
  styleUrl: './checkbox.component.css',
})
export class CheckboxComponent {
  /** Beschriftung neben der Box */
  @Input() label = '';
  /** Gewählter Zustand */
  @Input() checked = false;
  /** Deaktiviert die Checkbox */
  @Input() disabled = false;
  /** Emittiert den neuen Zustand */
  @Output() checkedChange = new EventEmitter<boolean>();

  toggle(event: Event): void {
    if (this.disabled) return;
    const next = (event.target as HTMLInputElement).checked;
    this.checkedChange.emit(next);
  }
}

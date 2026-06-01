import { Component, Input, Output, EventEmitter } from '@angular/core';
import { CommonModule } from '@angular/common';

/**
 * Eine Option im Select-Dropdown.
 *  value: Wert der zurück ans Parent emittiert wird
 *  label: Anzeigetext im Dropdown
 */
export interface SelectOption<T = string> {
  value: T;
  label: string;
}

/**
 * Wiederverwendbares Select-Dropdown.
 *
 * Beispiel:
 *   <app-select
 *     label="Fragetyp"
 *     [options]="typeOptions"
 *     [value]="currentType"
 *     (valueChange)="onTypeChange($event)" />
 */
@Component({
  selector: 'app-select',
  standalone: true,
  imports: [CommonModule],
  templateUrl: './select.component.html',
  styleUrl: './select.component.css',
})
export class SelectComponent<T extends string | number = string> {
  /** Optionales Label oberhalb des Selects */
  @Input() label = '';
  /** Hinweistext bei Fehler (zeigt Fehler-Styling) */
  @Input() error = '';
  /** Aktuell gewählter Wert */
  @Input() value: T | null = null;
  /** Liste der wählbaren Optionen */
  @Input() options: SelectOption<T>[] = [];
  /** Deaktiviert das Select */
  @Input() disabled = false;
  /** Emittiert sobald sich der Wert ändert */
  @Output() valueChange = new EventEmitter<T>();

  onChange(event: Event): void {
    const raw = (event.target as HTMLSelectElement).value;
    // Falls Optionen numerisch sind: zurück zu number konvertieren
    const isNumeric = this.options.length > 0 && typeof this.options[0].value === 'number';
    const next = (isNumeric ? Number(raw) : raw) as T;
    this.valueChange.emit(next);
  }
}

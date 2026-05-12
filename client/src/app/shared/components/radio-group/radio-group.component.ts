import { Component, Input, Output, EventEmitter } from '@angular/core';
import { CommonModule } from '@angular/common';

/** Eine Option in der Radio-Gruppe. */
export interface RadioOption<T = string | number> {
  value: T;
  label: string;
}

/**
 * Wiederverwendbare Radio-Button-Gruppe (Single Choice).
 *
 * Erzeugt einen eindeutigen `name`-Pool damit mehrere Gruppen auf einer Seite
 * unabhängig sind (sonst überschreiben sie sich gegenseitig).
 *
 * Beispiel:
 *   <app-radio-group
 *     [options]="[{value: 0, label: 'A'}, {value: 1, label: 'B'}]"
 *     [value]="selected()"
 *     (valueChange)="selected.set($event)" />
 */
let radioGroupCounter = 0;

@Component({
  selector: 'app-radio-group',
  standalone: true,
  imports: [CommonModule],
  templateUrl: './radio-group.component.html',
  styleUrl: './radio-group.component.css',
})
export class RadioGroupComponent<T extends string | number = string> {
  /** Optionales Label oberhalb der Gruppe */
  @Input() label = '';
  /** Liste der Auswahlmöglichkeiten */
  @Input() options: RadioOption<T>[] = [];
  /** Aktuell gewählter Wert (oder null wenn nichts ausgewählt) */
  @Input() value: T | null = null;
  /** Deaktiviert die ganze Gruppe */
  @Input() disabled = false;
  /** Emittiert den neu gewählten Wert */
  @Output() valueChange = new EventEmitter<T>();

  // Eindeutiger HTML-name damit mehrere Radio-Groups unabhängig funktionieren
  readonly name = `radio-group-${++radioGroupCounter}`;

  select(opt: RadioOption<T>): void {
    if (this.disabled) return;
    this.valueChange.emit(opt.value);
  }
}

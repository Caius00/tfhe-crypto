import { Component, EventEmitter, Input, Output } from '@angular/core';
import { CommonModule } from '@angular/common';
import { CardComponent } from '../../../../shared/components/card/card.component';
import { CheckboxComponent } from '../../../../shared/components/checkbox/checkbox.component';
import { RadioGroupComponent, RadioOption } from '../../../../shared/components/radio-group/radio-group.component';
import { InputComponent } from '../../../../shared/components/input/input.component';
import { Question } from '../../voting.types';

/**
 * Antwort-Wert für eine einzelne Frage.
 * Variante hängt vom Fragetyp ab:
 *   - bool:     boolean
 *   - single:   number   (Index der ausgewählten Option)
 *   - multiple: number[] (Indizes aller ausgewählten Optionen)
 *   - numeric:  number
 */
export type AnswerValue = boolean | number | number[] | undefined;

/**
 * Eingabe-Komponente für eine einzelne Voting-Frage.
 *
 * Wählt automatisch das richtige Eingabe-Element basierend auf `question.question_type`
 * und gibt den getippten Antwortwert als typsicheres Event nach oben.
 *
 * Wiederverwendbar für jeden Service der Fragen mit denselben Typen verwendet
 * (Statistics, Surveys, ...).
 */
@Component({
  selector: 'app-question-input',
  standalone: true,
  imports: [
    CommonModule,
    CardComponent,
    CheckboxComponent,
    RadioGroupComponent,
    InputComponent,
  ],
  templateUrl: './question-input.component.html',
  styleUrl: './question-input.component.css',
})
export class QuestionInputComponent {
  /** Die zu beantwortende Frage */
  @Input({ required: true }) question!: Question;
  /** 1-basierte Index-Anzeige */
  @Input() index = 1;
  /** Aktueller Antwortwert (typabhängig, siehe AnswerValue) */
  @Input() value: AnswerValue = undefined;

  /** Emittiert den neuen Antwortwert */
  @Output() valueChange = new EventEmitter<AnswerValue>();

  // --- Bool ----------------------------------------------------------------

  get boolValue(): boolean {
    return this.value === true;
  }

  onBoolChange(checked: boolean): void {
    this.valueChange.emit(checked);
  }

  // --- Numeric -------------------------------------------------------------

  get numericValue(): string {
    return typeof this.value === 'number' ? String(this.value) : '';
  }

  onNumericChange(raw: string): void {
    const n = Number(raw);
    this.valueChange.emit(Number.isFinite(n) ? n : 0);
  }

  // --- Single Choice -------------------------------------------------------

  get singleValue(): number | null {
    return typeof this.value === 'number' ? this.value : null;
  }

  /** Mappt q.options zu RadioOptions (label = Optionstext, value = Index) */
  get radioOptions(): RadioOption<number>[] {
    return (this.question.options ?? []).map((label, i) => ({ value: i, label }));
  }

  onSingleChange(idx: number): void {
    this.valueChange.emit(idx);
  }

  // --- Multiple Choice -----------------------------------------------------

  get multipleValue(): number[] {
    return Array.isArray(this.value) ? this.value : [];
  }

  isMultipleChecked(idx: number): boolean {
    return this.multipleValue.includes(idx);
  }

  onMultipleToggle(idx: number, checked: boolean): void {
    const set = new Set(this.multipleValue);
    if (checked) set.add(idx);
    else set.delete(idx);
    this.valueChange.emit(Array.from(set).sort());
  }
}

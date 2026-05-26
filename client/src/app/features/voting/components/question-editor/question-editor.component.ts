import { Component, EventEmitter, Input, Output } from '@angular/core';
import { CommonModule } from '@angular/common';
import { CardComponent } from '../../../../shared/components/card/card.component';
import { InputComponent } from '../../../../shared/components/input/input.component';
import { ButtonComponent } from '../../../../shared/components/button/button.component';
import { SelectComponent, SelectOption } from '../../../../shared/components/select/select.component';
import { Question, QuestionType } from '../../voting.types';

/**
 * Editor für eine einzelne Voting-Frage.
 *
 * Bekommt eine Question per Input und emittiert Änderungen über `questionChange`.
 * Der Parent hält die komplette Liste – diese Komponente ist stateless und
 * komplett wiederverwendbar (z.B. auch für Statistics, Surveys, ...).
 */
@Component({
  selector: 'app-question-editor',
  standalone: true,
  imports: [CommonModule, CardComponent, InputComponent, ButtonComponent, SelectComponent],
  templateUrl: './question-editor.component.html',
  styleUrl: './question-editor.component.css',
})
export class QuestionEditorComponent {
  /** Die zu bearbeitende Frage */
  @Input({ required: true }) question!: Question;
  /** 1-basierte Index-Anzeige ("Frage 1", "Frage 2", ...) */
  @Input() index = 1;
  /** Soll der "Entfernen"-Button angezeigt werden? */
  @Input() canRemove = true;

  /** Emittiert die geänderte Frage (immutable update) */
  @Output() questionChange = new EventEmitter<Question>();
  /** Wird beim Klick auf "Entfernen" emittiert */
  @Output() remove = new EventEmitter<void>();

  /** Auswahlmöglichkeiten für den Frage-Typ */
  readonly typeOptions: SelectOption<QuestionType>[] = [
    { value: 'single',   label: 'Single Choice' },
    { value: 'multiple', label: 'Multiple Choice' },
    { value: 'numeric',  label: 'Numerisch (0–255)' },
  ];

  /** Hat die aktuelle Frage Auswahloptionen (Single / Multiple)? */
  get hasOptions(): boolean {
    return this.question.question_type === 'single' || this.question.question_type === 'multiple';
  }

  // --- Handler: alle erzeugen eine neue Question und emittieren sie ----------

  onTextChange(text: string): void {
    this.questionChange.emit({ ...this.question, text });
  }

  onTypeChange(question_type: QuestionType): void {
    // Bei Wechsel auf single/multiple: Optionen-Liste anlegen falls leer
    const next: Question = {
      ...this.question,
      question_type,
      options: (question_type === 'single' || question_type === 'multiple')
        ? (this.question.options && this.question.options.length > 0 ? this.question.options : ['', ''])
        : null,
    };
    this.questionChange.emit(next);
  }

  onOptionChange(idx: number, value: string): void {
    if (!this.question.options) return;
    const options = this.question.options.map((o, i) => (i === idx ? value : o));
    this.questionChange.emit({ ...this.question, options });
  }

  addOption(): void {
    const options = [...(this.question.options ?? []), ''];
    this.questionChange.emit({ ...this.question, options });
  }

  removeOption(idx: number): void {
    if (!this.question.options) return;
    const options = this.question.options.filter((_, i) => i !== idx);
    this.questionChange.emit({ ...this.question, options });
  }
}

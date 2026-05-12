import { Component, Input, Output, EventEmitter } from '@angular/core';
import { FormsModule } from '@angular/forms';

@Component({
  selector: 'app-input',
  imports: [FormsModule],
  templateUrl: './input.component.html',
  styleUrl: './input.component.css',
})
export class InputComponent {
  @Input() label = '';
  @Input() placeholder = '';
  @Input() type: 'text' | 'number' | 'password' = 'text';
  @Input() value = '';
  @Output() valueChange = new EventEmitter<string>();

  onInput(val: string): void {
    this.value = val;
    this.valueChange.emit(val);
  }
}

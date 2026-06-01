import { Component, Input } from '@angular/core';
import { NgClass } from '@angular/common';

@Component({
  selector: 'app-alert',
  imports: [],
  templateUrl: './alert.component.html',
  styleUrl: './alert.component.css',
})
export class AlertComponent {
  @Input() message = '';
  @Input() variant: 'success' | 'error' | 'warning' | 'info' = 'info';

  icon(): string {
    return { success: '✓', error: '✕', warning: '!', info: 'i' }[this.variant];
  }
}

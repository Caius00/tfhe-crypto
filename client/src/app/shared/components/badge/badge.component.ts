import { Component, Input } from '@angular/core';
import { NgClass } from '@angular/common';

@Component({
  selector: 'app-badge',
  imports: [NgClass],
  template: `<span class="badge" [ngClass]="variant">{{ label }}</span>`,
  styleUrl: './badge.component.css',
})
export class BadgeComponent {
  @Input() label = '';
  @Input() variant: 'success' | 'error' | 'warning' | 'info' | 'neutral' = 'neutral';
}

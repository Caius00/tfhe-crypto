import { Component, Input } from '@angular/core';
import { NgClass } from '@angular/common';

@Component({
  selector: 'app-card',
  imports: [NgClass],
  template: `<div class="card" [ngClass]="variant"><ng-content /></div>`,
  styleUrl: './card.component.css',
})
export class CardComponent {
  @Input() variant: 'default' | 'elevated' | 'flat' | 'error' = 'default';
}

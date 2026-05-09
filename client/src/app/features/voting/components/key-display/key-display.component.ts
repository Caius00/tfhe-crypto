import { Component, computed, Input, signal } from '@angular/core';
import { CommonModule } from '@angular/common';
import { ButtonComponent } from '../../../../shared/components/button/button.component';
import { CopyButtonComponent } from '../../../../shared/components/copy-button/copy-button.component';

/** Ein einzelner anzuzeigender Schlüssel (Label + Base64-Wert + kurze Erklärung). */
export interface KeyEntry {
  label: string;
  value: string;
  description: string;
}

/**
 * Ausklappbares Anzeige-Element für die FHE-Schlüssel der Session.
 *
 * Aus Sicherheits-/UX-Gründen sind die Schlüssel **standardmäßig versteckt**.
 * Per Toggle pro Schlüssel kann der Inhalt sichtbar gemacht und kopiert werden.
 *
 * Wiederverwendbar für jeden Service der dem User Schlüssel zeigen will.
 */
@Component({
  selector: 'app-key-display',
  standalone: true,
  imports: [CommonModule, ButtonComponent, CopyButtonComponent],
  templateUrl: './key-display.component.html',
  styleUrl: './key-display.component.css',
})
export class KeyDisplayComponent {
  /** Anzuzeigende Schlüssel */
  @Input() keys: KeyEntry[] = [];

  /** Welche Schlüssel sind aktuell sichtbar (per Index) */
  visible = signal<Set<number>>(new Set());

  /** Nicht im Klartext, sondern abgekürzt anzeigen wenn nicht sichtbar */
  preview(value: string): string {
    if (!value) return '';
    if (value.length <= 32) return value;
    return value.slice(0, 16) + '…' + value.slice(-12);
  }

  isVisible(idx: number): boolean {
    return this.visible().has(idx);
  }

  toggle(idx: number): void {
    this.visible.update((set) => {
      const next = new Set(set);
      if (next.has(idx)) next.delete(idx);
      else next.add(idx);
      return next;
    });
  }

  /** Kurzanzeige der Längen-Info im Header */
  bytesHint = computed(() => this.keys.map((k) => Math.round((k.value.length * 3) / 4)));
}

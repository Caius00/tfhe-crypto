import { Component, Input, signal } from '@angular/core';

/**
 * Wiederverwendbarer Button zum Kopieren eines Werts in die Zwischenablage.
 *
 * Nutzt die Clipboard-API (navigator.clipboard.writeText). Zeigt nach
 * erfolgreichem Kopieren für 1.5s ein "Kopiert!" als visuelles Feedback.
 *
 * Beispiel:
 *   <app-copy-button [value]="sessionId" />
 *   <app-copy-button [value]="serverKeyB64" label="Server-Key kopieren" />
 */
@Component({
  selector: 'app-copy-button',
  standalone: true,
  templateUrl: './copy-button.component.html',
  styleUrl: './copy-button.component.css',
})
export class CopyButtonComponent {
  /** Der Wert der in die Zwischenablage kopiert werden soll */
  @Input({ required: true }) value!: string;
  /** Anzeigetext im Ruhezustand (Default: nur Icon) */
  @Input() label = '';
  /** Variante für den Stil */
  @Input() variant: 'inline' | 'pill' = 'inline';

  /** True für 1.5s nach erfolgreichem Kopieren – steuert das Feedback */
  copied = signal(false);
  /** True wenn Clipboard-API einen Fehler geworfen hat */
  failed = signal(false);

  async copy(): Promise<void> {
    if (!this.value) return;
    try {
      await navigator.clipboard.writeText(this.value);
      this.copied.set(true);
      this.failed.set(false);
      setTimeout(() => this.copied.set(false), 1500);
    } catch (e) {
      console.error('Clipboard write failed', e);
      this.failed.set(true);
      setTimeout(() => this.failed.set(false), 1500);
    }
  }
}

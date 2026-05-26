import { Component, Input } from '@angular/core';
import { SpinnerComponent } from '../spinner/spinner.component';
import { CryptoLoaderComponent } from '../crypto-loader/crypto-loader.component';

/**
 * Vollflächiger Loading-Overlay für blockierende Operationen
 * (z.B. Schlüsselgenerierung, Verschlüsselung großer Daten).
 *
 * Wird als Sibling/Overlay über den Inhalt gelegt. Anzeige steuert man
 * von außen über @if.
 *
 * Beispiel:
 *   @if (isGenerating()) {
 *     <app-loading-overlay message="Schlüssel werden erzeugt..." />
 *   }
 */
@Component({
  selector: 'app-loading-overlay',
  standalone: true,
  imports: [SpinnerComponent, CryptoLoaderComponent],
  templateUrl: './loading-overlay.component.html',
  styleUrl: './loading-overlay.component.css',
})
export class LoadingOverlayComponent {
  /** Hauptmeldung (über dem Spinner) */
  @Input() message = 'Lädt...';
  /** Optional: zusätzlicher Hinweistext */
  @Input() hint = '';
  /** false = block-zentriert (Inline), true = full screen */
  @Input() fullscreen = false;
  /**
   * 'crypto' = themed Krypto-Animation (Ringe + Schloss)
   * 'spinner' = simpler Spinner (für leichtere Operationen)
   */
  @Input() variant: 'crypto' | 'spinner' = 'crypto';
}

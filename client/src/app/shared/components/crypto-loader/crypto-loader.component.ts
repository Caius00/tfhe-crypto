import { Component, Input } from '@angular/core';

/**
 * Themed Lade-Animation für Krypto-Operationen.
 *
 * Zeigt drei konzentrische Ringe (mit unterschiedlichen Geschwindigkeiten und
 * Drehrichtungen), drei orbitende Punkte und ein zentrales Schloss-Icon, das
 * pulsiert. Wirkt gehaltvoller als ein einfacher Spinner und passt thematisch
 * zur Verschlüsselung.
 *
 * Reine CSS-Animation – keine JS-Renderkosten.
 *
 * Beispiel:
 *   <app-crypto-loader />
 *   <app-crypto-loader size="lg" />
 */
@Component({
  selector: 'app-crypto-loader',
  standalone: true,
  templateUrl: './crypto-loader.component.html',
  styleUrl: './crypto-loader.component.css',
})
export class CryptoLoaderComponent {
  /** Größe der Animation */
  @Input() size: 'sm' | 'md' | 'lg' = 'md';
}

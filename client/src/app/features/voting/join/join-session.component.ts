import { Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { CommonModule } from '@angular/common';
import { firstValueFrom } from 'rxjs';

import { TfheService } from '../../../core/crypto/tfhe.service';
import { VotingService } from '../voting.service';

import { PageHeaderComponent } from '../../../shared/components/page-header/page-header.component';
import { CardComponent } from '../../../shared/components/card/card.component';
import { InputComponent } from '../../../shared/components/input/input.component';
import { ButtonComponent } from '../../../shared/components/button/button.component';
import { AlertComponent } from '../../../shared/components/alert/alert.component';

/**
 * Maske zum Beitreten einer existierenden Voting-Session.
 *
 * Ablauf:
 *   1) Session-ID + Name eingeben
 *   2) Public-Key der Session laden (vom Server)
 *   3) Name lokal verschlüsseln (Compact-Public-Key)
 *   4) Join-Request senden → Redirect zur Warte-Seite
 */
@Component({
  selector: 'app-join-session',
  standalone: true,
  imports: [
    CommonModule,
    PageHeaderComponent,
    CardComponent,
    InputComponent,
    ButtonComponent,
    AlertComponent,
  ],
  templateUrl: './join-session.component.html',
  styleUrl: './join-session.component.css',
})
export class JoinSessionComponent {
  private votingService = inject(VotingService);
  private router = inject(Router);
  private tfhe = inject(TfheService);

  sessionId = signal('');
  name = signal('');

  isJoining = signal(false);
  errorMessage = signal<string | null>(null);

  /** Eingaben gültig? (für Submit-Button) */
  canSubmit = computed(() => !!this.sessionId().trim() && !!this.name().trim());

  /**
   * Sendet den Beitritts-Request.
   * Holt den Public-Key der Session vom Server (falls nicht im sessionStorage),
   * verschlüsselt den Namen lokal und navigiert dann zur Warte-Seite.
   */
  async join(): Promise<void> {
    if (!this.canSubmit()) return;
    this.errorMessage.set(null);
    this.isJoining.set(true);

    try {
      const participantId = 'p-' + crypto.randomUUID();
      localStorage.setItem('participantId', participantId);

      await this.tfhe.ensureInitialized();

      // Public-Key per-Session cachen – sonst würde beim Wechsel zwischen
      // mehreren Sessions der falsche Key benutzt.
      const sid = this.sessionId().trim();
      const cacheKey = `tfhe_public_key_${sid}`;
      let publicKeyB64 = sessionStorage.getItem(cacheKey);
      if (!publicKeyB64) {
        const session = await firstValueFrom(this.votingService.getSession(sid));
        if (!session?.public_key) {
          throw new Error('Session ungültig oder ohne Public-Key.');
        }
        publicKeyB64 = session.public_key;
        // Cache ist optional. Wenn das Quota überschritten wird (Public-Key
        // ist mehrere MB), funktioniert der Join trotzdem – nur ohne Cache.
        try {
          sessionStorage.setItem(cacheKey, publicKeyB64);
        } catch (e) {
          console.warn('Public-Key konnte nicht gecached werden (Quota?)', e);
        }
      }

      // Name verschlüsseln (Batch via CompactCiphertextList) und an Backend senden.
      // Vorher: N einzelne FHE-Verschlüsselungen pro Zeichen → 1–3 s/Zeichen.
      // Jetzt: alle Zeichen in einer einzigen Operation → ~5–10× schneller.
      const encNameChunks = this.tfhe.encryptStringCompact(publicKeyB64, this.name().trim());
      await firstValueFrom(
        this.votingService.joinSession(sid, participantId, encNameChunks),
      );

      this.router.navigate(['/voting/waiting', sid]);
    } catch (e) {
      console.error('Join failed', e);
      this.errorMessage.set(
        'Beitritt fehlgeschlagen. Prüfe die Session-ID und versuche es erneut.',
      );
      this.isJoining.set(false);
    }
  }
}

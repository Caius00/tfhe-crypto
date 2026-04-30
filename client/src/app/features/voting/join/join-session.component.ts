// src/app/voting/components/join-session.component.ts
import { Component, signal, inject } from '@angular/core';
import { Router } from '@angular/router';
import { VotingService } from '../voting.service';
import { InputComponent } from '../../../shared/components/input/input.component';
import { ButtonComponent } from '../../../shared/components/button/button.component';
import { CommonModule } from '@angular/common';
import { TfheService } from '../../../core/crypto/tfhe.service';
import { firstValueFrom } from 'rxjs';

@Component({
  standalone: true,
  imports: [InputComponent, ButtonComponent, CommonModule],
  templateUrl: './join-session.component.html',
})
export class JoinSessionComponent {
  private votingService = inject(VotingService);
  private router = inject(Router);
  private tfhe = inject(TfheService);

  sessionId = signal('');
  participantId = signal('');

  get hasPublicKey(): boolean {
    try {
      return !!sessionStorage.getItem('tfhe_public_key');
    } catch {
      return false;
    }
  }

// inside JoinSessionComponent
async join() {
  if (!this.sessionId() || !this.participantId()) return;

  localStorage.setItem('participantId', this.participantId());

  await this.tfhe.ensureInitialized();

  // 1) Versuche publicKey aus sessionStorage zu laden
  let publicB64 = sessionStorage.getItem('tfhe_public_key') || null;

  // 2) Falls nicht vorhanden, lade Session-Metadaten vom Server
  if (!publicB64) {
    try {
      // firstValueFrom wandelt das Observable in ein Promise um
      const session = await firstValueFrom(this.votingService.getSession(this.sessionId()));

      // session kann theoretisch undefined sein; prüfe daher sicherheitshalber
      if (session && session.public_key) {
        publicB64 = session.public_key;
        sessionStorage.setItem('tfhe_public_key', publicB64);
      } else {
        console.error('Session hat keinen public_key oder Session nicht gefunden', session);
      }
    } catch (err) {
      console.error('Fehler beim Laden der Session-Metadaten', err);
    }
  }

  // 3) Wenn immer noch kein publicKey vorhanden ist, abbrechen
  if (!publicB64) {
    this.router.navigate(['/voting/error']); // optional: UI-Fehlerseite oder Meldung
    console.error('Kein Public Key verfügbar. Beitritt nicht möglich.');
    return;
  }

  // 4) Name abfragen und verschlüsseln
  const name = prompt('Gib deinen Namen ein') || this.participantId();
  const encNameChunks = this.tfhe.encryptStringWithPublic(publicB64, name);

  // 5) Join-Request an Backend senden
  this.votingService.joinSession(this.sessionId(), this.participantId(), encNameChunks)
    .subscribe({
      next: () => {
        this.router.navigate(['/voting/waiting', this.sessionId()]);
      },
      error: err => {
        console.error('JOIN ERROR', err);
      }
    });
}

}

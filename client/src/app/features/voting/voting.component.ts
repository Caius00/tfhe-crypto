import { Component, signal, inject } from '@angular/core';
import { VotingService } from '../../features/voting/voting.service';
import { TfheService } from '../../core/crypto/tfhe.service';
import { TfheClientKey } from 'tfhe';
import { InputComponent } from '../../shared/components/input/input.component';
import { ButtonComponent } from '../../shared/components/button/button.component';
import { KeyPair } from '../../core/crypto/key-pair.model';


@Component({
  selector: 'app-voting',
  imports: [InputComponent, ButtonComponent],
  templateUrl: './voting.component.html',
})
export class VotingComponent {
  private votingService = inject(VotingService);
  private tfhe = inject(TfheService);

  // Keys – bleiben lokal im Speicher
  private clientKey: TfheClientKey | null = null;
  private keyPair: KeyPair | null= null;
  // Eingaben
  sessionId = signal('');
  participantId = signal('');
  creatorId = signal('');
  voteValue = signal(true); // true = Ja, false = Nein
  // Status
  keyStatus = signal('');
  createStatus = signal('');
  createdSessionId = signal('');
  joinStatus = signal('');
  voteStatus = signal('');
  pendingList = signal<string[]>([]);
  results = signal<{ encrypted_results: string[]; ready: boolean } | null>(null);
  decryptedResults = signal<number[]>([]);

  // Schritt 1: Keys generieren
  async generateKeys(): Promise<void> {
    this.keyStatus.set('Keys werden generiert...');
    await new Promise((resolve) => setTimeout(resolve, 50));

    try {
      await this.tfhe.ensureInitialized();
      // Schlüssel nur im Speicher
      this.keyPair = this.tfhe.generateKeyPair();
      this.keyStatus.set('Keys generiert und gespeichert');
    } catch (e) {
      console.error('Key-Generierung Fehler:', e); // ← neu
      this.keyStatus.set('Fehler: ' + (e as Error).message);
    }
  }

  createSession(): void {
    if (!this.keyPair) {
      this.createStatus.set('Zuerst Keys generieren!');
      return;
    }

    const serverKeyB64 = this.tfhe.toBase64(this.keyPair.serverKeyBytes);

    this.votingService
      .createSession(this.creatorId(), serverKeyB64, [
        {
          id: 1,
          text: 'Soll das Projekt fortgesetzt werden?',
          question_type: 'bool',
          options: null,
        },
      ])
      .subscribe({
        next: (res) => {
          this.createdSessionId.set(res.session_id);
          this.sessionId.set(res.session_id);
          this.createStatus.set('Session erstellt');
        },
        error: () => this.createStatus.set('Fehler beim Erstellen der Session'),
      });
  }
  joinSession(): void {
    this.votingService.joinSession(this.sessionId(), this.participantId()).subscribe({
      next: (res) => this.joinStatus.set('Beigetreten – Status: ' + res.status),
      error: () => this.joinStatus.set('Fehler beim Beitreten'),
    });
  }

  getPending(): void {
    this.votingService.getPending(this.sessionId(), this.creatorId()).subscribe({
      next: (list) => this.pendingList.set(list),
      error: () => this.pendingList.set([]),
    });
  }

  approve(participantId: string): void {
    this.votingService
      .approveParticipant(this.sessionId(), this.creatorId(), participantId, true)
      .subscribe({ next: () => this.getPending() });
  }

  reject(participantId: string): void {
    this.votingService
      .approveParticipant(this.sessionId(), this.creatorId(), participantId, false)
      .subscribe({ next: () => this.getPending() });
  }

  // Schritt 5: Abstimmen
  submitVote(): void {
    if (!this.keyPair) {
      this.voteStatus.set('Kein ClientKey gefunden!');
      return;
    }

    const encryptedBytes = this.tfhe.encryptBool(this.voteValue(), this.keyPair.clientKey);
    const encryptedB64 = this.tfhe.toBase64(encryptedBytes);

    this.votingService
      .submitVote(this.sessionId(), this.participantId(), [encryptedB64])
      .subscribe({
        next: (res) => this.voteStatus.set('erfolgreich abgestimmt ' + res.status),
        error: () => this.voteStatus.set('Fehler beim Abstimmen'),
      });
  }

  // Schritt 6: Ergebnisse abrufen + entschlüsseln
  getResults(): void {
    this.votingService.getResults(this.sessionId(), this.creatorId()).subscribe({
      next: (res) => {
        this.results.set(res);
        if (res.ready) {
          this.decryptResults(res.encrypted_results);
        }
      },
      error: () => this.results.set(null),
    });
  }

  private decryptResults(encryptedResults: string[]): void {
    if (!this.keyPair) return;

    const decrypted = encryptedResults.map((b64) => {
      const bytes = this.tfhe.fromBase64(b64);
      return this.tfhe.decryptUint8(bytes, this.keyPair!.clientKey);
    });

    this.decryptedResults.set(decrypted);
  }
}

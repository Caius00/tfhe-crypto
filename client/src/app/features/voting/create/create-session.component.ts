// src/app/voting/components/create-session.component.ts
import { Component, signal, inject } from '@angular/core';
import { Router } from '@angular/router';
import { CommonModule } from '@angular/common';
import { TfheService } from '../../../core/crypto/tfhe.service';
import { VotingService } from '../voting.service';
import { Question, QuestionType } from '../voting.types';

@Component({
  standalone: true,
  imports: [CommonModule],
  templateUrl: './create-session.component.html',
})
export class CreateSessionComponent {
  private votingService = inject(VotingService);
  private tfhe = inject(TfheService);
  private router = inject(Router);

  creatorId = signal('');
  status = signal('');
  sessionId = signal<string | null>(null);

  questions = signal<Question[]>([
    { id: 1, text: '', question_type: 'bool', options: null, multiple: false },
  ]);

  private keyPair: any | null = null;

  // falls QuestionType als TS-Enum oder string-union existiert, importiere ihn oben:
  // import { QuestionType } from '...';

  onQuestionTypeChange(index: number, newType: string) {
    // Mappe den string auf den erwarteten QuestionType
    // Falls QuestionType ein string-union ist ('bool'|'single'|'multiple'|'numeric'),
    // ist das casten in der Regel ausreichend:
    const qt = newType as unknown as QuestionType;

    this.questions.update((arr) => {
      const copy = [...arr];
      copy[index] = { ...copy[index], question_type: qt };
      return copy;
    });
  }

  public serverKeyB64: string | null = null;
  public publicKeyB64: string | null = null;

  // nachdem du keyPair gesetzt hast (z.B. in generateKeys()):
  private setKeyPair(kp: any) {
    this.keyPair = kp;
    if (kp) {
      // benutze deinen TfheService, falls vorhanden
      this.serverKeyB64 = this.tfhe.toBase64(kp.serverKeyBytes);
      this.publicKeyB64 = this.tfhe.toBase64(kp.publicKeyBytes);
    } else {
      this.serverKeyB64 = null;
      this.publicKeyB64 = null;
    }
  }

  // Getter für Template (public)
  get serverKeyBase64(): string {
    return this.keyPair ? this.tfhe.toBase64(this.keyPair.serverKeyBytes) : '';
  }
  get publicKeyBase64(): string {
    return this.keyPair && this.keyPair.publicKeyBytes
      ? this.tfhe.toBase64(this.keyPair.publicKeyBytes)
      : '';
  }

  get hasKeyPair(): boolean {
    return !!this.keyPair;
  }

  // generateKeys: setze keyPair und speichere in IndexedDB / sessionStorage
  async generateKeys() {
    this.status.set('Generating keys...');
    await new Promise((resolve) => setTimeout(resolve, 50));
    try {
      await this.tfhe.ensureInitialized();
      this.keyPair = this.tfhe.generateKeyPair();
      this.status.set('Keys ready');
    } catch (e) {
      console.error('Key error:', e);
      this.status.set('Fehler: ' + (e as Error).message);
    }
  }

  updateQuestion(i: number, value: string) {
    this.questions.update((q) => {
      const copy = [...q];
      copy[i] = { ...copy[i], text: value };
      return copy;
    });
  }

  addQuestion() {
    this.questions.update((q) => [
      ...q,
      { id: q.length + 1, text: '', question_type: 'bool', options: null, multiple: false },
    ]);
  }

  addOption(qIndex: number) {
    this.questions.update((q) => {
      const copy = [...q];
      const qItem = { ...copy[qIndex] };
      qItem.options = qItem.options ? [...qItem.options, ''] : [''];
      copy[qIndex] = qItem;
      return copy;
    });
  }

  updateOption(qIndex: number, optIndex: number, value: string) {
    this.questions.update((q) => {
      const copy = [...q];
      const qItem = { ...copy[qIndex] };
      qItem.options = qItem.options
        ? qItem.options.map((o, i) => (i === optIndex ? value : o))
        : null;
      copy[qIndex] = qItem;
      return copy;
    });
  }

  toggleMultiple(qIndex: number) {
    this.questions.update((q) => {
      const copy = [...q];
      copy[qIndex] = { ...copy[qIndex], multiple: !copy[qIndex].multiple };
      return copy;
    });
  }

  create() {
    if (!this.keyPair || !this.creatorId()) {
      this.status.set('Zuerst Keys generieren');
      return;
    }

    const serverKeyB64 = this.tfhe.toBase64(this.keyPair.serverKeyBytes);
    const publicKeyB64 = this.keyPair.publicKeyBytes
      ? this.tfhe.toBase64(this.keyPair.publicKeyBytes)
      : null;

    this.votingService.createSession(this.creatorId(), serverKeyB64, publicKeyB64, this.questions())
      .subscribe({
        next: res => {
          // ClientKey in Redis speichern
          const clientKeyBytes = this.keyPair!.clientKey.serialize();
          const clientKeyB64 = this.tfhe.toBase64(clientKeyBytes);

          this.votingService.storeClientKey(res.session_id, clientKeyB64).subscribe({
            next: () => {
              this.sessionId.set(res.session_id);
              localStorage.setItem('creatorId', this.creatorId());
              this.router.navigateByUrl(`/voting/manage/${res.session_id}`);
            },
            error: () => this.status.set('Fehler beim Speichern des ClientKeys')
          });
        },
        error: () => this.status.set('Fehler beim Erstellen der Session')
      });
  }
}


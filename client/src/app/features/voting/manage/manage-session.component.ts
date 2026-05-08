// src/app/voting/components/manage-session.component.ts
import { Component, signal, inject, OnInit, OnDestroy } from '@angular/core';
import { ActivatedRoute, Router } from '@angular/router';
import { VotingService } from '../voting.service';
import { TfheService } from '../../../core/crypto/tfhe.service';
import { CommonModule } from '@angular/common';
import { ButtonComponent } from '../../../shared/components/button/button.component';

@Component({
  standalone: true,
  imports: [CommonModule, ButtonComponent],
  templateUrl: './manage-session.component.html',
})
export class ManageSessionComponent implements OnInit, OnDestroy {
  private route = inject(ActivatedRoute);
  private votingService = inject(VotingService);
  private tfhe = inject(TfheService);
  private router = inject(Router);

  questions = signal<any[]>([]);

  sessionId = '';
  creatorId = localStorage.getItem('creatorId') || '';

  pending = signal<{ participant_id: string; enc_name_chunks: string[] }[]>([]);
  results = signal<(string | string[])[]>([]);
  decryptedResults = signal<any[]>([]);
  status = signal<string>('bereit');
  decryptedNames = signal<Record<string, string>>({});

  private intervalId: any;
  private privateKey: any | null = null;

  async ngOnInit() {
    this.sessionId = this.route.snapshot.paramMap.get('sessionId') || '';
    await this.tfhe.ensureInitialized();

    // Session laden (FRAGEN!)
    this.votingService.getSession(this.sessionId).subscribe({
      next: (res) => {
        this.questions.set(res.questions || []);
      },
      error: (err) => {
        console.error('Failed to load session', err);
      },
    });

    // ClientKey aus SessionStorage laden
    const storedKey = sessionStorage.getItem(`clientKey_${this.sessionId}`);

    if (storedKey) {
      try {
        const bytes = this.tfhe.fromBase64(storedKey);
        const { TfheClientKey } = await import('tfhe');
        this.privateKey = TfheClientKey.deserialize(bytes);

        this.getPending();
        this.intervalId = setInterval(() => this.getPending(), 2000);
      } catch (e) {
        console.error('Fehler beim Laden des ClientKeys', e);
        this.status.set('ClientKey konnte nicht geladen werden');
      }
    } else {
      this.status.set('Kein ClientKey im SessionStorage gefunden');
    }
  }

  ngOnDestroy() {
    clearInterval(this.intervalId);
  }

  isArray(value: any): value is string[] {
    return Array.isArray(value);
  }

  getPending() {
    this.votingService.getPending(this.sessionId, this.creatorId).subscribe(
      (res) => {
        // Expect res: [{ participant_id, enc_name_chunks }]
        this.pending.set(res);
      },
      (err) => {
        console.error('Failed to fetch pending', err);
      },
    );
  }

  decryptName(encChunks: string[] | undefined): string {
    if (!encChunks || encChunks.length === 0) return '(kein Name)';

    if (!this.privateKey) {
      console.error('Kein privater Schlüssel zum Entschlüsseln vorhanden');
      return '(kein Schlüssel)';
    }

    try {
      const bytes: number[] = [];

      for (const chunkB64 of encChunks) {
        // Base64 -> raw bytes (Ciphertext)
        const raw = this.tfhe.fromBase64(chunkB64);

        // decryptUint8 erwartet (bytes, clientKey) und liefert eine Zahl 0..255
        // Falls dein Server wirklich pro Chunk mehrere Klartext-Bytes liefert,
        // wird dieser Aufruf fehlschlagen und du musst serverseitiges Format anpassen.
        const val = this.tfhe.decryptUint8(raw, this.privateKey);
        bytes.push(val);
      }

      return new TextDecoder().decode(new Uint8Array(bytes));
    } catch (e) {
      console.error('Decrypt name failed', e);
      return 'Fehler beim Entschlüsseln';
    }
  }

  decryptNameForParticipant(participantId: string, encChunks: string[] | undefined): void {
    if (!encChunks || encChunks.length === 0) {
      this.decryptedNames.update((n) => ({ ...n, [participantId]: '(kein Name)' }));
      return;
    }
    if (!this.privateKey) {
      this.decryptedNames.update((n) => ({ ...n, [participantId]: '(kein Schlüssel)' }));
      return;
    }
    try {
      const bytes: number[] = [];
      for (const chunkB64 of encChunks) {
        const raw = this.tfhe.fromBase64(chunkB64);
        const val = this.tfhe.decryptUint8(raw, this.privateKey);
        bytes.push(val);
      }
      const name = new TextDecoder().decode(new Uint8Array(bytes));
      this.decryptedNames.update((n) => ({ ...n, [participantId]: name }));
    } catch (e) {
      console.error('Decrypt name failed', e);
      this.decryptedNames.update((n) => ({ ...n, [participantId]: '(Fehler)' }));
    }
  }

  approve(id: string) {
    this.votingService
      .approveParticipant(this.sessionId, this.creatorId, id, true)
      .subscribe(() => this.getPending());
  }

  reject(id: string) {
    this.votingService
      .approveParticipant(this.sessionId, this.creatorId, id, false)
      .subscribe(() => this.getPending());
  }

  loadResults() {
    this.status.set('Lade Ergebnisse...');
    this.decryptedResults.set([]);
    this.votingService.getResults(this.sessionId, this.creatorId).subscribe(
      (res) => {
        console.log('=== RESULTS VOM BACKEND ===');
        console.log('ready:', res.ready);
        console.log('encrypted_results:', res.encrypted_results);
        console.log('Anzahl:', res.encrypted_results.length);
        res.encrypted_results.forEach((r, i) => {
          console.log(`Ergebnis ${i} Länge:`, r.length);
          console.log(`Ergebnis ${i}:`, r);
        });
        this.results.set(res.encrypted_results);
        this.status.set(res.ready ? 'Fertig' : 'Waiting');
      },
      (err) => {
        console.error('Failed to load results', err);
        this.status.set('Fehler beim Laden');
      },
    );
  }

  decryptFinalResults() {
    if (!this.privateKey) return;

    const clientKey = this.privateKey;
    const questions = this.questions();

    const decrypted = this.results().map((questionResult: string | string[], qIndex: number) => {
      const question = questions[qIndex];

      if (!question) return '(Frage fehlt)';
      if (!questionResult || questionResult.length === 0) return '(keine Daten)';

      // BOOL / NUMERIC
      if (question.question_type === 'bool' || question.question_type === 'numeric') {
        try {
          const raw = this.tfhe.fromBase64(questionResult[0]);
          const num = this.tfhe.decryptUint8(raw, clientKey);

          return question.question_type === 'bool' ? `Ja: ${num}` : num.toString();
        } catch (e) {
          console.error('Decrypt Fehler:', e);
          return '(Fehler)';
        }
      }

      // SINGLE / MULTIPLE
      if (question.question_type === 'single' || question.question_type === 'multiple') {
        const options = question.options || [];

        return options.map((opt: string, i: number) => {
          try {
            const enc = questionResult[i];
            if (!enc) return `${opt}: 0`;

            const raw = this.tfhe.fromBase64(enc);
            const count = this.tfhe.decryptUint8(raw, clientKey);

            return `${opt}: ${count}`;
          } catch (e) {
            console.error(`Fehler bei Option ${i}`, e);
            return `${opt}: (Fehler)`;
          }
        });
      }

      return '(Unbekannter Typ)';
    });

    this.decryptedResults.set(decrypted);
  }

  finalize() {
    this.votingService.finalizeSession(this.sessionId, this.creatorId).subscribe({
      next: () => {
        this.status.set('Session beendet');
        setTimeout(() => {
          this.router.navigate(['/voting']);
        }, 1500);
      },
      error: (err) => {
        console.error(err);
        this.status.set('Fehler beim Beenden');
      },
    });
  }
}

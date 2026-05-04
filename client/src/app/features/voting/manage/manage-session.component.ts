// src/app/voting/components/manage-session.component.ts
import { Component, signal, inject, OnInit, OnDestroy } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
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

  sessionId = '';
  creatorId = localStorage.getItem('creatorId') || '';

  pending = signal<{ participant_id: string; enc_name_chunks: string[] }[]>([]);
  results = signal<string[]>([]);
  status = signal<string>('bereit');

  private intervalId: any;
  private privateKey: any | null = null;

  async ngOnInit() {
    this.sessionId = this.route.snapshot.paramMap.get('sessionId') || '';
    await this.tfhe.ensureInitialized();

    // ClientKey aus Redis laden
    this.votingService.loadClientKey(this.sessionId).subscribe({
      next: async (res) => {
        const bytes = this.tfhe.fromBase64(res.client_key);
        const { TfheClientKey } = await import('tfhe');
        this.privateKey = TfheClientKey.deserialize(bytes);
        this.getPending();
        this.intervalId = setInterval(() => this.getPending(), 2000);
      },
      error: () => {
        this.status.set('Kein ClientKey gefunden');
      },
    });
  }

  ngOnDestroy() {
    clearInterval(this.intervalId);
  }

  // in ManageSessionComponent
  isArray(value: any): boolean {
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
    this.votingService.getResults(this.sessionId, this.creatorId).subscribe(
      (res) => {
        console.log('=== RESULTS VOM BACKEND ===');
        console.log('ready:', res.ready);
        console.log('encrypted_results:', res.encrypted_results);
        console.log('Anzahl:', res.encrypted_results.length);
        res.encrypted_results.forEach((r, i) => {
          console.log(`Ergebnis ${i} Länge:`, r.length);
          console.log(`Ergebnis ${i} Anfang:`, r.substring(0, 50));
        });
        this.results.set(res.encrypted_results);
        this.status.set(res.ready ? 'Fertig' : '⏳ Waiting');
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

    console.log('=== RESULTS DEBUG ===');
    console.log('Rohdaten:', this.results());

    const decrypted = this.results().map((enc, i) => {
      console.log(`Ergebnis ${i}:`, enc.substring(0, 100));
      const raw = this.tfhe.fromBase64(enc);
      console.log(`Byte-Länge ${i}:`, raw.length);

      try {
        const num = this.tfhe.decryptUint8(raw, clientKey);
        console.log(`decryptUint8 ${i}:`, num);
        return num.toString();
      } catch (e1) {
        console.error(`decryptUint8 Fehler ${i}:`, e1);
        try {
          const bool = this.tfhe.decryptBool(raw, clientKey);
          console.log(`decryptBool ${i}:`, bool);
          return bool ? 'Ja' : 'Nein';
        } catch (e2) {
          console.error(`decryptBool Fehler ${i}:`, e2);
          return '(Fehler)';
        }
      }
    });

    this.results.set(decrypted);
  }

  finalize() {
    this.votingService.finalizeSession(this.sessionId, this.creatorId).subscribe({
      next: () => this.status.set('Session beendet'),
      error: (err) => {
        console.error(err);
        this.status.set('Fehler beim Beenden');
      },
    });
  }
}

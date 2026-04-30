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
    this.privateKey = await this.tfhe.loadKeyPairFromSession();
    if (!this.privateKey) {
      console.error('No private key found');
      this.status.set('Kein privater Schlüssel vorhanden');
      return;
    }
    this.getPending();
    this.intervalId = setInterval(() => this.getPending(), 2000);
  }

  ngOnDestroy() {
    clearInterval(this.intervalId);
  }

  // in ManageSessionComponent
isArray(value: any): boolean {
  return Array.isArray(value);
}


  getPending() {
    this.votingService.getPending(this.sessionId, this.creatorId)
      .subscribe(res => {
        // Expect res: [{ participant_id, enc_name_chunks }]
        this.pending.set(res);
      }, err => {
        console.error('Failed to fetch pending', err);
      });
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
    this.votingService.approveParticipant(this.sessionId, this.creatorId, id, true)
      .subscribe(() => this.getPending());
  }

  reject(id: string) {
    this.votingService.approveParticipant(this.sessionId, this.creatorId, id, false)
      .subscribe(() => this.getPending());
  }

  loadResults() {
    this.status.set('Lade Ergebnisse...');
    this.votingService.getResults(this.sessionId, this.creatorId)
      .subscribe(res => {
        this.results.set(res.encrypted_results);
        this.status.set(res.ready ? '✅ Fertig' : '⏳ Waiting');
      }, err => {
        console.error('Failed to load results', err);
        this.status.set('❌ Fehler beim Laden');
      });
  }

  decryptFinalResults() {
    if (!this.privateKey) return;
    const decrypted = this.results().map(enc => {
      const raw = this.tfhe.fromBase64(enc);
      try {
        const bool = this.tfhe.decryptBool(raw, this.privateKey);
        return bool ? 'true' : 'false';
      } catch {
        const num = this.tfhe.decryptUint8(raw, this.privateKey);
        return num.toString();
      }
    });
    this.results.set(decrypted);
  }

  finalize() {
    this.votingService.finalizeSession(this.sessionId, this.creatorId)
      .subscribe({
        next: () => this.status.set('Session beendet'),
        error: (err) => { console.error(err); this.status.set('❌ Fehler beim Beenden'); }
      });
  }
}

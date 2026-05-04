// src/app/voting/components/vote.component.ts
import { Component, signal, inject, OnInit } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
import { VotingService } from '../voting.service';
import { TfheService } from '../../../core/crypto/tfhe.service';
import { CommonModule } from '@angular/common';

@Component({
  standalone: true,
  imports: [CommonModule],
  templateUrl: './vote.component.html',
})
export class VoteComponent implements OnInit {
  private route = inject(ActivatedRoute);
  private votingService = inject(VotingService);
  private tfhe = inject(TfheService);

  sessionId = '';
  questions = signal<any[]>([]);
   voteValue = signal<(boolean | number | Set<number> | undefined)[]>([]);
  voteStatus = signal<string | null>(null);

  private publicKeyB64: string | null = null;

    async ngOnInit() {
    this.sessionId = this.route.snapshot.paramMap.get('sessionId') || '';
    await this.tfhe.ensureInitialized();

    this.publicKeyB64 = sessionStorage.getItem('tfhe_public_key') || null;

    this.votingService.getSession(this.sessionId).subscribe({
      next: res => {
        this.questions.set(res.questions || []);
        if (!this.publicKeyB64 && res.public_key) {
          this.publicKeyB64 = res.public_key;
          sessionStorage.setItem('tfhe_public_key', res.public_key);
        }
        // optional: ensure voteValue has same length as questions
        const qlen = (res.questions || []).length;
        this.voteValue.set(new Array(qlen).fill(undefined));
      },
      error: err => {
        console.error('Failed to load session metadata', err);
      }
    });
  }

    toggleBool(i: number) {
    this.voteValue.update(v => {
      const copy = [...v];
      const current = copy[i] as boolean | undefined;
      copy[i] = !current;
      return copy;
    });
  }

 onNumericInput(qIndex: number, value: string) {
    const num = Number(value);
    this.voteValue.update(v => {
      const copy = [...v];
      copy[qIndex] = num;
      return copy;
    });
  }


   selectSingle(qIndex: number, optIndex: number) {
    this.voteValue.update(v => {
      const copy = [...v];
      copy[qIndex] = optIndex;
      return copy;
    });
  }

  toggleMultiple(i: number, oi: number) {
    this.voteValue.update(arr => {
      const copy = [...arr];
      const current = copy[i] as Set<number> | undefined;
      const next = new Set<number>(current ?? []);
      if (next.has(oi)) next.delete(oi); else next.add(oi);
      copy[i] = next;
      return copy;
    });
  }

  isOptionChecked(i: number, oi: number): boolean {
    const arr = this.voteValue();
    const set = arr && arr[i] ? (arr[i] as Set<number> | undefined) : undefined;
    return !!set && set.has(oi);
  }

    private generateParticipantId() {
    return 'p-' + Math.random().toString(36).slice(2, 10);
  }

  async submit() {
    if (!this.publicKeyB64) {
      this.voteStatus.set('Kein Public Key verfügbar');
      return;
    }

    const participantId = localStorage.getItem('participantId') || this.generateParticipantId();
    localStorage.setItem('participantId', participantId);

    // 1) Name verschlüsseln -> Array von Base64-Chunks (string[])
    const name = prompt('Gib deinen Namen ein') || participantId;
    const encNameChunks = this.tfhe.encryptStringWithPublic(this.publicKeyB64, name);
    // encNameChunks ist bereits string[] (Base64)

    // 2) Antworten verschlüsseln -> array of Base64 strings
    const encryptedVotes: string[] = this.questions().map((q, i) => {
      if (q.question_type === 'bool') {
        const value = !!this.voteValue()[i];
        const encBytes = this.tfhe.encryptBoolWithPublic(this.publicKeyB64!, value);
        return this.tfhe.toBase64(encBytes);
      } else if (q.question_type === 'single') {
        const val = this.voteValue()[i];
        const sel = typeof val === 'number' ? val : 0;
        const encBytes = this.tfhe.encryptUint8WithPublic(this.publicKeyB64!, sel);
        return this.tfhe.toBase64(encBytes);
      } else if (q.question_type === 'multiple') {
        const set = this.voteValue()[i] as Set<number> | undefined;
        let mask = 0;
        if (set) {
          for (const idx of Array.from(set)) mask |= 1 << idx;
        }
        const encBytes = this.tfhe.encryptUint8WithPublic(this.publicKeyB64!, mask);
        return this.tfhe.toBase64(encBytes);
      } else if (q.question_type === 'numeric') {
        const val = this.voteValue()[i];
        const num = typeof val === 'number' ? val : Number(val) || 0;
        const encBytes = this.tfhe.encryptUint8WithPublic(this.publicKeyB64!, num);
        return this.tfhe.toBase64(encBytes);
      }
      return '';
    });

    // 3) Join mit verschlüsseltem Namen (encNameChunks: string[])
    this.voteStatus.set(' Warte auf Genehmigung...');
    this.votingService.joinSession(this.sessionId, participantId, encNameChunks).subscribe({
      next: () => {
        // 2) Pollen bis genehmigt
        this.pollUntilApproved(participantId, encryptedVotes);
      },
      error: (err) => {
        console.error('JOIN ERROR', err);
        this.voteStatus.set('Join fehlgeschlagen');
      },
    });
  }

  private pollUntilApproved(participantId: string, encryptedVotes: string[]) {
    const interval = setInterval(() => {
      this.votingService.getStatus(this.sessionId, participantId).subscribe({
        next: (res) => {
          if (res.status === 'approved') {
            clearInterval(interval);
            this.voteStatus.set('Genehmigt – sende Stimme...');
            this.sendVote(participantId, encryptedVotes);
          } else if (res.status === 'not_found') {
            clearInterval(interval);
            this.voteStatus.set('Teilnehmer nicht gefunden');
          }
          // 'pending' → weiter warten
        },
        error: () => clearInterval(interval)
      });
    }, 2000); // alle 2 Sekunden prüfen
  }

  private sendVote(participantId: string, encryptedVotes: string[]) {
    this.votingService.submitVote(this.sessionId, participantId, encryptedVotes)
      .subscribe({
        next: () => this.voteStatus.set('Stimme erfolgreich abgegeben!'),
        error: err => {
          console.error(err);
          this.voteStatus.set('Stimme fehlgeschlagen');
        }
      });
  }
}


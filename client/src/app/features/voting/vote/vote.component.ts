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

    const participantId = localStorage.getItem('participantId');
    if (!participantId) {
      this.voteStatus.set('Keine Teilnehmer-ID gefunden');
      return;
    }

    // Votes verschlüsseln
    const encryptedVotes: string[] = this.questions().map((q, i) => {
      if (q.question_type === 'bool') {
        const value = !!this.voteValue()[i];
        return this.tfhe.toBase64(this.tfhe.encryptBoolWithPublic(this.publicKeyB64!, value));
      } else if (q.question_type === 'single') {
        const val = typeof this.voteValue()[i] === 'number' ? this.voteValue()[i] as number : 0;
        return this.tfhe.toBase64(this.tfhe.encryptUint8WithPublic(this.publicKeyB64!, val));
      } else if (q.question_type === 'multiple') {
        const set = this.voteValue()[i] as Set<number> | undefined;
        let mask = 0;
        if (set) for (const idx of Array.from(set)) mask |= 1 << idx;
        return this.tfhe.toBase64(this.tfhe.encryptUint8WithPublic(this.publicKeyB64!, mask));
      } else if (q.question_type === 'numeric') {
        const num = typeof this.voteValue()[i] === 'number' ? this.voteValue()[i] as number : 0;
        return this.tfhe.toBase64(this.tfhe.encryptUint8WithPublic(this.publicKeyB64!, num));
      }
      return '';
    });

    // Direkt abstimmen – Teilnehmer ist bereits genehmigt
    this.voteStatus.set('Stimme wird gesendet...');
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


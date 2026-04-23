import { Component, signal, inject } from '@angular/core';
import { VotingService } from '../../features/voting/voting.service';
import { InputComponent } from '../../shared/components/input/input.component';
import { ButtonComponent } from '../../shared/components/button/button.component';

@Component({
  selector: 'app-voting',
  imports: [InputComponent, ButtonComponent],
  templateUrl: './voting.component.html',
})
export class VotingComponent {
  private votingService = inject(VotingService);

  // Eingaben
  serverKey = signal('');
  createdSessionId = signal('');
  createError = signal('');
  sessionId = signal('');
  participantId = signal('');
  creatorId = signal('');
  encryptedVote = signal('');

  // Status
  joinStatus = signal('');
  voteStatus = signal('');
  pendingList = signal<string[]>([]);
  results = signal<{ encrypted_results: string[]; ready: boolean } | null>(null);

  createSession(): void {
    this.votingService
      .createSession(this.creatorId(), this.serverKey(), [
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
          this.sessionId.set(res.session_id); // automatisch übernehmen
          this.createError.set('');
        },
        error: () => this.createError.set('Ungültiger ServerKey oder Fehler beim Erstellen'),
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

  submitVote(): void {
    this.votingService
      .submitVote(this.sessionId(), this.participantId(), [this.encryptedVote()])
      .subscribe({
        next: (res) => this.voteStatus.set('abgestimmt ' + res.status),
        error: () => this.voteStatus.set('Fehler beim Abstimmen'),
      });
  }

  getResults(): void {
    this.votingService.getResults(this.sessionId(), this.creatorId()).subscribe({
      next: (res) => this.results.set(res),
      error: () => this.results.set(null),
    });
  }
}

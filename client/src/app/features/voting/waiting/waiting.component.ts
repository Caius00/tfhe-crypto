import { Component, inject, OnInit, OnDestroy } from '@angular/core';
import { ActivatedRoute, Router } from '@angular/router';
import { VotingService } from '../voting.service';
import { CommonModule } from '@angular/common';

@Component({
  standalone: true,
  imports: [CommonModule],
  templateUrl: './waiting.component.html',
})
export class WaitingComponent implements OnInit, OnDestroy {
  private route = inject(ActivatedRoute);
  private votingService = inject(VotingService);
  private router = inject(Router);

  sessionId = '';
  private intervalId: any;

  ngOnInit() {
    this.sessionId = this.route.snapshot.paramMap.get('sessionId') || '';

    this.intervalId = setInterval(() => {
      const participantId = localStorage.getItem('participantId');

      if (!this.sessionId || !participantId) return;

      this.votingService.getStatus(this.sessionId, participantId)
        .subscribe({
          next: res => {
            if (res.status === 'approved') {
              clearInterval(this.intervalId);
              this.router.navigate(['/voting/vote', this.sessionId]);
            }
          },
          error: err => {
            console.error('STATUS ERROR', err);
          }
        });

    }, 2000);
  }

  ngOnDestroy() {
    clearInterval(this.intervalId);
  }
}
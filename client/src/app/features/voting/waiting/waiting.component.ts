import { Component, OnDestroy, OnInit, inject, signal } from '@angular/core';
import { ActivatedRoute, Router } from '@angular/router';
import { CommonModule } from '@angular/common';
import { VotingService } from '../voting.service';

import { PageHeaderComponent } from '../../../shared/components/page-header/page-header.component';
import { CardComponent } from '../../../shared/components/card/card.component';
import { SpinnerComponent } from '../../../shared/components/spinner/spinner.component';
import { AlertComponent } from '../../../shared/components/alert/alert.component';
import { ButtonComponent } from '../../../shared/components/button/button.component';

const POLL_INTERVAL_MS = 2000;

/**
 * Warte-Seite für Teilnehmer.
 *
 * Pollt alle 2 Sekunden den Status. Sobald der Ersteller den Teilnehmer freigibt,
 * wird automatisch zur Vote-Seite weitergeleitet.
 */
@Component({
  selector: 'app-waiting',
  standalone: true,
  imports: [
    CommonModule,
    PageHeaderComponent,
    CardComponent,
    SpinnerComponent,
    AlertComponent,
    ButtonComponent,
  ],
  templateUrl: './waiting.component.html',
  styleUrl: './waiting.component.css',
})
export class WaitingComponent implements OnInit, OnDestroy {
  private route = inject(ActivatedRoute);
  private votingService = inject(VotingService);
  private router = inject(Router);

  /** Anzahl bisheriger Status-Abfragen (für UI-Hinweis) */
  attempts = signal(0);
  /** Fehler beim Polling */
  errorMessage = signal<string | null>(null);

  private sessionId = '';
  private pollTimer: any = null;

  ngOnInit(): void {
    this.sessionId = this.route.snapshot.paramMap.get('sessionId') ?? '';
    if (!this.sessionId) {
      this.errorMessage.set('Keine Session-ID in der URL.');
      return;
    }

    // Erste Abfrage sofort, dann im Intervall – sonst sieht der Nutzer
    // 2 Sekunden lang gar nichts und denkt, die Seite hängt.
    this.checkStatus();
    this.pollTimer = setInterval(() => this.checkStatus(), POLL_INTERVAL_MS);
  }

  ngOnDestroy(): void {
    if (this.pollTimer) clearInterval(this.pollTimer);
  }

  /** Bricht das Warten ab und navigiert zurück */
  cancel(): void {
    if (this.pollTimer) clearInterval(this.pollTimer);
    this.router.navigate(['/voting']);
  }

  private checkStatus(): void {
    const participantId = localStorage.getItem('participantId');
    if (!participantId) {
      this.errorMessage.set('Keine Teilnehmer-ID gefunden.');
      return;
    }

    this.attempts.update((n) => n + 1);
    this.votingService.getStatus(this.sessionId, participantId).subscribe({
      next: (res) => {
        if (res.status === 'approved') {
          if (this.pollTimer) clearInterval(this.pollTimer);
          this.router.navigate(['/voting/vote', this.sessionId]);
        } else if (res.status === 'not_found') {
          if (this.pollTimer) clearInterval(this.pollTimer);
          this.errorMessage.set(
            'Du wurdest aus der Session entfernt. Bitte erneut beitreten.',
          );
        }
      },
      error: () => {
        // Stiller Fehler – nur loggen damit Polling weiterläuft
        console.warn('Status check failed');
      },
    });
  }
}

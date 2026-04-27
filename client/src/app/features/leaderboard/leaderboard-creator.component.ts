import { Component, Input, Output, EventEmitter } from '@angular/core';
import { ButtonComponent } from '../../shared/components/button/button.component';
import { SpinnerComponent } from '../../shared/components/spinner/spinner.component';

export interface DecryptedEntry {
  rank: number;
  score: number;
  playerId: string;
}

@Component({
  selector: 'app-leaderboard-creator',
  imports: [ButtonComponent, SpinnerComponent],
  templateUrl: './leaderboard-creator.component.html',
  styleUrl: './leaderboard-creator.component.css',
})
export class LeaderboardCreatorComponent {
  @Input() roomCode = '';
  @Input() entries: DecryptedEntry[] = [];
  @Input() loading = false;
  @Input() lastUpdated: Date | null = null;
  @Output() refresh = new EventEmitter<void>();

  medal(rank: number): string {
    if (rank === 1) return '🥇';
    if (rank === 2) return '🥈';
    if (rank === 3) return '🥉';
    return `#${rank}`;
  }

  timeAgo(): string {
    if (!this.lastUpdated) return '';
    const secs = Math.round((Date.now() - this.lastUpdated.getTime()) / 1000);
    if (secs < 5) return 'gerade eben';
    if (secs < 60) return `vor ${secs}s`;
    return `vor ${Math.round(secs / 60)}min`;
  }
}

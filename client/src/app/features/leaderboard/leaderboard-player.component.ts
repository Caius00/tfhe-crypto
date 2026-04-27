import { Component, Input, Output, EventEmitter } from '@angular/core';
import { FlappyBirdComponent } from '../../shared/components/flappy-bird/flappy-bird.component';

@Component({
  selector: 'app-leaderboard-player',
  imports: [FlappyBirdComponent],
  templateUrl: './leaderboard-player.component.html',
  styleUrl: './leaderboard-player.component.css',
})
export class LeaderboardPlayerComponent {
  @Input() roomCode = '';
  @Input() playerId = '';
  @Output() submitScore = new EventEmitter<number>();

  onGameOver(score: number): void {
    this.submitScore.emit(score);
  }
}

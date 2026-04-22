import { Component } from '@angular/core';
import { FlappyBirdComponent } from '../../shared/components/flappy-bird/flappy-bird.component';

@Component({
  selector: 'app-leaderboard',
  imports: [FlappyBirdComponent],
  templateUrl: './leaderboard.component.html',
  styleUrl: './leaderboard.component.css',
})
export class LeaderboardComponent {}

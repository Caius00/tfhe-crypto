import { Component, Output, EventEmitter, signal } from '@angular/core';
import { ButtonComponent } from '../../shared/components/button/button.component';
import { InputComponent } from '../../shared/components/input/input.component';

@Component({
  selector: 'app-leaderboard-landing',
  imports: [ButtonComponent, InputComponent],
  templateUrl: './leaderboard-landing.component.html',
  styleUrl: './leaderboard-landing.component.css',
})
export class LeaderboardLandingComponent {
  @Output() createRoom = new EventEmitter<void>();
  @Output() joinRoom = new EventEmitter<string>();

  joinCode = signal('');

  onJoin(): void {
    const code = this.joinCode().trim();
    if (code.length === 6 && /^\d{6}$/.test(code)) {
      this.joinRoom.emit(code);
    }
  }
}

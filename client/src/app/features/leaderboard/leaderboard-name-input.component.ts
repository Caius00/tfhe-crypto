import { Component, EventEmitter, Input, Output, computed, signal } from '@angular/core';
import { ButtonComponent } from '../../shared/components/button/button.component';
import { InputComponent } from '../../shared/components/input/input.component';
import { PlayerIdentityService } from './player-identity.service';

/**
 * Name-Dialog nach erfolgreichem Beitritt in einen neuen Raum.
 *
 * Wird vom übergeordneten `LeaderboardComponent` NUR beim ersten Beitritt
 * eingeblendet — bei Re-Joins liest dieses die Identität direkt aus dem
 * `PlayerIdentityService` (localStorage) und springt direkt in die Player-View.
 */
@Component({
  selector: 'app-leaderboard-name-input',
  imports: [ButtonComponent, InputComponent],
  templateUrl: './leaderboard-name-input.component.html',
  styleUrl: './leaderboard-name-input.component.css',
})
export class LeaderboardNameInputComponent {
  @Input() roomCode = '';
  @Output() nameSubmitted = new EventEmitter<string>();

  name = signal('');

  /** Live-Validierung — steuert Button-Disabled und Hint-Anzeige. */
  readonly isValid = computed(() => this.identity.isValidName(this.name().trim()));

  constructor(private identity: PlayerIdentityService) {}

  onSubmit(): void {
    if (!this.isValid()) return;
    this.nameSubmitted.emit(this.name().trim());
  }
}

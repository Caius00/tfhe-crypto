import { Component, signal } from '@angular/core';
import { InputComponent } from '../../shared/components/input/input.component';
import { ButtonComponent } from '../../shared/components/button/button.component';

@Component({
  selector: 'app-voting',
  imports: [InputComponent, ButtonComponent],
  templateUrl: './voting.component.html',
})
export class VotingComponent {
  sessionId = signal('');
  vote = signal('');

  onSubmit(): void {
    console.log('Session:', this.sessionId(), '| Vote:', this.vote());
  }
}

import { Component } from '@angular/core';
import { Router } from '@angular/router';
import { CommonModule } from '@angular/common';
import { ButtonComponent } from '../../../shared/components/button/button.component';

@Component({
  selector: 'app-voting-home',
  standalone: true,
  imports: [ButtonComponent, CommonModule],
  templateUrl: './voting-home.component.html',
})
export class VotingHomeComponent {
  constructor(private router: Router) {}

  create() {
    this.router.navigate(['/voting/create']);
  }

  join() {
    this.router.navigate(['/voting/join']);
  }
}
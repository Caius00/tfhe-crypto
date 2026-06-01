import { Component, inject } from '@angular/core';
import { Router } from '@angular/router';
import { PageHeaderComponent } from '../../../shared/components/page-header/page-header.component';
import { CardComponent } from '../../../shared/components/card/card.component';
import { ButtonComponent } from '../../../shared/components/button/button.component';

/**
 * Einstiegsseite für das Voting-Feature.
 * Zwei Hauptaktionen: neue Session erstellen ODER bestehender Session beitreten.
 */
@Component({
  selector: 'app-voting-home',
  standalone: true,
  imports: [PageHeaderComponent, CardComponent, ButtonComponent],
  templateUrl: './voting-home.component.html',
  styleUrl: './voting-home.component.css',
})
export class VotingHomeComponent {
  private router = inject(Router);

  /** Navigiert zur Erstellungsmaske einer neuen Voting-Session */
  goCreate(): void {
    this.router.navigate(['/voting/create']);
  }

  /** Navigiert zur Beitritts-Maske für eine bestehende Session */
  goJoin(): void {
    this.router.navigate(['/voting/join']);
  }
}

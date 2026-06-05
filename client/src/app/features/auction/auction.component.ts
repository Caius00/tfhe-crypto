import { Component } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { FormsModule } from '@angular/forms';
import { CommonModule } from '@angular/common';

@Component({
  selector: 'app-auction',
  standalone: true,
  imports: [CommonModule, FormsModule],
  templateUrl: './auction.component.html',
  styleUrls: [],
})
export class AuctionComponent {
  bidderName: string = '';
  bidAmount: number = 0;
  serverKey: string = 'dGVzdA==';

  statusMessage: string = '';
  evaluationResult: string = '';

  constructor(private http: HttpClient) {}

  // 1. Gebot absenden anBackend
  sendBid() {
    const payload = {
      bidder_name: this.bidderName,

      encrypted_amount: btoa(this.bidAmount.toString()),
      server_key: this.serverKey,
    };

    this.http.post<any>('/api/gebot', payload).subscribe({
      next: (res) => (this.statusMessage = res.response),
      error: (err) =>
        (this.statusMessage = 'Fehler beim Senden: ' + (err.error?.message || err.statusText)),
    });
  }

  evaluateAuction() {
    this.http.get<any>('/api/auswerten').subscribe({
      next: (res) => (this.evaluationResult = res.status),
      error: (err) =>
        (this.evaluationResult =
          'Fehler bei Auswertung: ' + (err.error?.message || err.statusText)),
    });
  }
}

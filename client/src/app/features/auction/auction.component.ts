import { Component, signal, OnInit } from '@angular/core';
import { HttpClient, HttpClientModule } from '@angular/common/http';
import { FormsModule } from '@angular/forms';
import { CommonModule } from '@angular/common';

import { TfheService } from '../../core/crypto/tfhe.service';
import { KeyPair } from '../../core/crypto/key-pair.model';

type AuctionStep = 'init' | 'generating' | 'ready' | 'sending' | 'evaluating' | 'result' | 'error';

interface LocalBid {
  name: string;
  amount: number;
}

@Component({
  selector: 'app-auction',
  standalone: true,
  imports: [CommonModule, FormsModule, HttpClientModule],
  templateUrl: './auction.component.html',
  styleUrls: [],
})
export class AuctionComponent implements OnInit {
  step = signal<AuctionStep>('init');
  bidderName = signal('');
  bidAmount = signal('');
  statusMessage = signal('');
  evaluationResult = signal('');

  private keyPair: KeyPair | null = null;

  private localBidsList: LocalBid[] = [];

  constructor(
    private http: HttpClient,
    private tfhe: TfheService,
  ) {}

  async ngOnInit() {
    this.step.set('generating');
    try {
      await this.tfhe.ensureInitialized();
      this.keyPair = this.tfhe.generateKeyPair();

      this.statusMessage.set('TFHE-Schlüssel erfolgreich im Browser generiert!');
      this.step.set('ready');
    } catch (e) {
      this.statusMessage.set('Fehler beim Initialisieren der Kryptographie.');
      this.step.set('error');
    }
  }

  // 1. Gebot ECHT als 32-Bit verschlüsseln und senden
  sendBid() {
    const amountValue = parseInt(this.bidAmount(), 10);
    // Erhöht auf das maximale Limit für 32-Bit signed Integers im Frontend-Check, falls nötig
    if (isNaN(amountValue) || amountValue < 0) {
      this.statusMessage.set('Bitte ein gültiges Gebot eingeben.');
      return;
    }
    if (!this.keyPair) return;

    this.step.set('sending');

    const currentName = this.bidderName();
    const currentAmount = amountValue;

    try {
      const encryptedAmountBytes = this.tfhe.encryptUint32(amountValue, this.keyPair.clientKey);

      const encryptedAmountB64 = this.tfhe.toBase64(encryptedAmountBytes);
      const serverKeyB64 = this.tfhe.toBase64(this.keyPair.serverKeyBytes);

      const payload = {
        bidder_name: currentName,
        encrypted_amount: encryptedAmountB64,
        server_key: serverKeyB64,
      };

      console.log('Payload size MB:', (JSON.stringify(payload).length / 1024 / 1024).toFixed(2));

      this.http.post<any>('/auction/gebot', payload).subscribe({
        next: (res) => {
          this.statusMessage.set(`Erfolg: Gebot von ${currentName} erfolgreich empfangen!`);

          this.localBidsList.push({ name: currentName, amount: currentAmount });

          this.step.set('ready');
          this.bidderName.set('');
          this.bidAmount.set('');
        },
        error: (err) => {
          this.statusMessage.set(`Fehler beim Senden: ${err.error?.message || err.statusText}`);
          this.step.set('error');
        },
      });
    } catch (e) {
      this.statusMessage.set('Fehler bei der Verschlüsselung.');
      this.step.set('error');
    }
  }

  evaluateAuction() {
    if (!this.keyPair) return;
    if (this.localBidsList.length === 0) {
      this.evaluationResult.set('Fehler: Es wurden lokal noch keine Gebote abgegeben!');
      return;
    }

    this.step.set('evaluating');

    this.http.get<any>('/auction/auswerten').subscribe({
      next: (res) => {
        const resultBytes = this.tfhe.fromBase64(res.encrypted_result);

        const highestBidAmount = this.tfhe.decryptUint32(resultBytes, this.keyPair!.clientKey);

        const winner = this.localBidsList.find((bid) => bid.amount === highestBidAmount);

        if (winner) {
          this.evaluationResult.set(
            `Auswertung fertig (${res.status}). \n` +
              `🏆 Gewinner: ${winner.name} mit einem anonymen Höchstgebot von ${highestBidAmount}€!`,
          );
        } else {
          this.evaluationResult.set(
            `Auswertung fertig (${res.status}). \n` +
              `Das Höchstgebot beträgt ${highestBidAmount}€, konnte aber lokal keinem Bieter zugeordnet werden.`,
          );
        }
        this.step.set('result');
      },
      error: (err) => {
        this.evaluationResult.set(`Fehler bei Auswertung: ${err.error?.message || err.statusText}`);
        this.step.set('error');
      },
    });
  }
}

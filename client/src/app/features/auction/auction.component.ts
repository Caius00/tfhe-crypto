import { Component, signal, OnInit } from '@angular/core';
import { HttpClient, HttpClientModule } from '@angular/common/http';
import { FormsModule } from '@angular/forms';
import { CommonModule } from '@angular/common';

// Import des hauseigenen Krypto-Services und der Modelle
import { TfheService } from '../../core/crypto/tfhe.service';
import { KeyPair } from '../../core/crypto/key-pair.model';

type AuctionStep = 'init' | 'generating' | 'ready' | 'sending' | 'evaluating' | 'result' | 'error';

// Interface für unsere geheime, rein lokale Bieterliste im Browser
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

  // HIER: Das lokale Array, um am Ende den Gewinnernamen zuzuordnen
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

  // Gebot ECHT verschlüsseln und absenden
  sendBid() {
    const amountValue = parseInt(this.bidAmount(), 10);
    if (isNaN(amountValue) || amountValue < 0) {
      this.statusMessage.set('Bitte ein gültiges Gebot eingeben.');
      return;
    }
    if (!this.keyPair) return;

    this.step.set('sending');

    // Wir merken uns den Namen und Betrag für die spätere Zuordnung im Erfolgsfall
    const currentName = this.bidderName();
    const currentAmount = amountValue;

    try {
      const encryptedAmountBytes = this.tfhe.encryptUint32
        ? this.tfhe.encryptUint32(amountValue, this.keyPair.clientKey)
        : this.tfhe.encryptUint8(amountValue, this.keyPair.clientKey);

      const encryptedAmountB64 = this.tfhe.toBase64(encryptedAmountBytes);
      const serverKeyB64 = this.tfhe.toBase64(this.keyPair.serverKeyBytes);

      const payload = {
        bidder_name: currentName,
        encrypted_amount: encryptedAmountB64,
        server_key: serverKeyB64,
      };

      this.http.post<any>('/auction/gebot', payload).subscribe({
        next: (res) => {
          // KORREKTUR: Antwort vom Server anzeigen (oder Fallback, falls res.response leer ist)
          const serverResponse =
            res && res.response ? res.response : `Gebot von ${currentName} erfolgreich empfangen!`;
          this.statusMessage.set(`Erfolg: ${serverResponse}`);

          // WICHTIG: Das Gebot lokal im Browser-RAM speichern
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

  // Blinde homomorphe Auswertung starten
  evaluateAuction() {
    if (!this.keyPair) return;
    if (this.localBidsList.length === 0) {
      this.evaluationResult.set('Fehler: Es wurden lokal noch keine Gebote registriert!');
      return;
    }

    this.step.set('evaluating');

    this.http.get<any>('/auction/auswerten').subscribe({
      next: (res) => {
        // Verschlüsseltes Ergebnis vom Rust-Server abholen
        const resultBytes = this.tfhe.fromBase64(res.encrypted_result);

        // KORREKTUR: Wir entschlüsseln jetzt eine Zahl (FheUint32) statt eines Bools!
        const highestBidAmount = this.tfhe.decryptUint32
          ? this.tfhe.decryptUint32(resultBytes, this.keyPair!.clientKey)
          : this.tfhe.decryptUint8(resultBytes, this.keyPair!.clientKey);

        // Suchen, wer diesen Betrag in unserer lokalen Liste abgegeben hat
        const winner = this.localBidsList.find((bid) => bid.amount === highestBidAmount);

        if (winner) {
          this.evaluationResult.set(
            `Auswertung fertig (${res.status}). \n` +
              `🏆 Gewinner: ${winner.name} mit einem anonymen Höchstgebot von ${highestBidAmount}!`,
          );
        } else {
          this.evaluationResult.set(
            `Auswertung fertig (${res.status}). \n` +
              `Das Höchstgebot beträgt ${highestBidAmount}, konnte jedoch lokal keinem Bieter zugeordnet werden.`,
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

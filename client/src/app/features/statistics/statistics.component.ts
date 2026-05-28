import { Component, signal } from '@angular/core';
import { TfheService } from '../../core/crypto/tfhe.service';
import { StatisticsApiService } from '../../core/api/statistics-api.service';
import { KeyPair } from '../../core/crypto/key-pair.model';
import { ButtonComponent } from '../../shared/components/button/button.component';
import { InputComponent } from '../../shared/components/input/input.component';
import { SpinnerComponent } from '../../shared/components/spinner/spinner.component';

type Step = 'init' | 'generating' | 'enter-list' | 'computing' | 'result' | 'error';

export interface StatisticsResult {
  sum: bigint;
  count: number;
  min: number;
  max: number;
  average: bigint;
  median: number;
}

@Component({
  selector: 'app-statistics',
  imports: [ButtonComponent, InputComponent, SpinnerComponent],
  templateUrl: './statistics.component.html',
})
export class StatisticsComponent {
  step = signal<Step>('init');
  listInput = signal('');
  result = signal<StatisticsResult | null>(null);
  errorMessage = signal('');

  private keyPair: KeyPair | null = null;

  constructor(
    private tfhe: TfheService,
    private api: StatisticsApiService
  ) {}

  async generateKeys(): Promise<void> {
    this.step.set('generating');
    await new Promise((resolve) => setTimeout(resolve, 50));

    try {
      await this.tfhe.ensureInitialized();
      this.keyPair = this.tfhe.generateKeyPair();
      this.step.set('enter-list');
    } catch {
      this.errorMessage.set('Fehler beim Generieren der Schlüssel.');
      this.step.set('error');
    }
  }

  async compute(): Promise<void> {
    // Komma-getrennte Eingabe parsen
    let numbers: number[];
    try {
      numbers = this.listInput()
        .split(',')
        .map((s) => s.trim())
        .filter((s) => s.length > 0)
        .map((s) => {
          const n = Number(s);
          if (!Number.isInteger(n)) throw new Error(`Kein gültiger Wert: "${s}"`);
          if (n < -2147483648 || n > 2147483647)
            throw new Error(`Wert außerhalb des i32-Bereichs [-2147483648, 2147483647]: "${s}"`);
          return n;
        });
    } catch (e: any) {
      this.errorMessage.set(e.message ?? 'Ungültige Eingabe.');
      this.step.set('error');
      return;
    }

    if (numbers.length === 0) {
      this.errorMessage.set('Bitte mindestens eine Zahl eingeben.');
      this.step.set('error');
      return;
    }
    if (!this.keyPair) return;

    this.step.set('computing');
    await new Promise((resolve) => setTimeout(resolve, 50));

    try {
      // Jede Zahl als FheInt32 verschlüsseln
      const encryptedList = numbers.map((n) => {
        const bytes = this.tfhe.encryptInt32(n, this.keyPair!.clientKey);
        return this.tfhe.toBase64(bytes);
      });
      const serverKeyB64 = this.tfhe.toBase64(this.keyPair.serverKeyBytes);

      this.api.compute(encryptedList, serverKeyB64).subscribe({
        next: (res) => {
          const sum     = this.tfhe.decryptInt64(this.tfhe.fromBase64(res.sum),     this.keyPair!.clientKey);
          const min     = this.tfhe.decryptInt32(this.tfhe.fromBase64(res.min),     this.keyPair!.clientKey);
          const max     = this.tfhe.decryptInt32(this.tfhe.fromBase64(res.max),     this.keyPair!.clientKey);
          const average = this.tfhe.decryptInt64(this.tfhe.fromBase64(res.average), this.keyPair!.clientKey);
          const median  = this.tfhe.decryptInt32(this.tfhe.fromBase64(res.median),  this.keyPair!.clientKey);

          this.result.set({ sum, count: res.count, min, max, average, median });
          this.step.set('result');
        },
        error: (err) => {
          this.errorMessage.set(`Server-Fehler: ${err.message ?? 'Unbekannter Fehler'}`);
          this.step.set('error');
        },
      });
    } catch (e: any) {
      this.errorMessage.set(e.message ?? 'Fehler bei der Verschlüsselung.');
      this.step.set('error');
    }
  }

  computeAnother(): void {
    this.result.set(null);
    this.step.set('enter-list');
  }

  reset(): void {
    this.keyPair = null;
    this.listInput.set('');
    this.result.set(null);
    this.errorMessage.set('');
    this.step.set('init');
  }
}

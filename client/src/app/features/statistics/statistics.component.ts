import { Component, signal } from '@angular/core';
import { TfheService } from '../../core/crypto/tfhe.service';
import { StatisticsApiService } from '../../core/api/statistics-api.service';
import { KeyPair } from '../../core/crypto/key-pair.model';
import { ButtonComponent } from '../../shared/components/button/button.component';
import { InputComponent } from '../../shared/components/input/input.component';
import { SpinnerComponent } from '../../shared/components/spinner/spinner.component';

type WorkflowStep = 'init' | 'generating' | 'enter-list' | 'computing' | 'result' | 'error';

export interface StatisticsResult {
  sum: number;
  count: number;
  min: number;
  max: number;
  average: number;
  median: number;
  bitWidth: 8 | 16 | 32;
}

/**
 * Wählt die kleinstmögliche FHE-Bitbreite anhand des tatsächlichen Wertebereichs
 * der Eingabeliste. Kleinere Bitbreiten bedeuten deutlich kürzere Rechenzeiten.
 *
 *   Int8  → [-128, 127]
 *   Int16 → [-32.768, 32.767]
 *   Int32 → [-2.147.483.648, 2.147.483.647]
 */
export function selectOptimalBitWidth(numbers: number[]): 8 | 16 | 32 {
  const smallestValue = Math.min(...numbers);
  const largestValue  = Math.max(...numbers);

  if (smallestValue >= -128 && largestValue <= 127)        return 8;
  if (smallestValue >= -32_768 && largestValue <= 32_767)  return 16;
  return 32;
}

/**
 * Steuert den vollständigen Statistics-Workflow:
 * Schlüsselgenerierung → Eingabe → homomorphe Berechnung → Ergebnisanzeige.
 *
 * Die Bitbreite der FHE-Verschlüsselung wird automatisch anhand des
 * Wertebereichs der Eingabe gewählt (Int8 / Int16 / Int32), um die
 * Berechnungszeit zu minimieren.
 */
@Component({
  selector: 'app-statistics',
  imports: [ButtonComponent, InputComponent, SpinnerComponent],
  templateUrl: './statistics.component.html',
})
export class StatisticsComponent {
  currentStep          = signal<WorkflowStep>('init');
  rawListInput         = signal('');
  computationResult    = signal<StatisticsResult | null>(null);
  validationError      = signal('');
  computeDurationMs    = signal<number | null>(null);
  displayedInputNumbers = signal<number[]>([]);

  private activeKeyPair: KeyPair | null = null;
  private computationStartTimestamp = 0;

  constructor(
    private readonly tfheService: TfheService,
    private readonly statisticsApiService: StatisticsApiService,
  ) {}

  /** Generiert ein neues TFHE-Schlüsselpaar (Client-Key + Server-Key). */
  async generateKeys(): Promise<void> {
    this.currentStep.set('generating');
    await new Promise((resolve) => setTimeout(resolve, 50));

    try {
      await this.tfheService.ensureInitialized();
      this.activeKeyPair = this.tfheService.generateKeyPair();
      this.currentStep.set('enter-list');
    } catch {
      this.validationError.set('Fehler beim Generieren der Schlüssel.');
      this.currentStep.set('error');
    }
  }

  /**
   * Parst die Komma-getrennte Eingabe, wählt die optimale Bitbreite,
   * verschlüsselt jeden Wert und schickt die Liste an den Server.
   */
  async compute(): Promise<void> {
    let parsedNumbers: number[];
    try {
      parsedNumbers = this.parseAndValidateInput(this.rawListInput());
    } catch (parseError: any) {
      this.validationError.set(parseError.message ?? 'Ungültige Eingabe.');
      this.currentStep.set('enter-list');
      return;
    }

    if (parsedNumbers.length === 0) {
      this.validationError.set('Bitte mindestens eine Zahl eingeben.');
      this.currentStep.set('enter-list');
      return;
    }
    if (!this.activeKeyPair) return;

    const selectedBitWidth = this.selectOptimalBitWidth(parsedNumbers);

    this.displayedInputNumbers.set(parsedNumbers);
    this.validationError.set('');
    this.computationStartTimestamp = Date.now();
    this.currentStep.set('computing');
    await new Promise((resolve) => setTimeout(resolve, 50));

    try {
      const encryptedNumberList = parsedNumbers.map((plainNumber) => {
        const encryptedBytes = this.encryptWithBitWidth(plainNumber, selectedBitWidth);
        return this.tfheService.toBase64(encryptedBytes);
      });
      const serverKeyBase64 = this.tfheService.toBase64(this.activeKeyPair.serverKeyBytes);

      this.statisticsApiService
        .compute(encryptedNumberList, serverKeyBase64, selectedBitWidth)
        .subscribe({
          next: (encryptedStatisticsResponse) => {
            const decryptedResult = this.decryptStatisticsResponse(
              encryptedStatisticsResponse,
              selectedBitWidth,
            );
            this.computeDurationMs.set(Date.now() - this.computationStartTimestamp);
            this.computationResult.set(decryptedResult);
            this.currentStep.set('result');
          },
          error: (httpError) => {
            this.validationError.set(
              `Server-Fehler: ${httpError.error ?? httpError.message ?? 'Unbekannter Fehler'}`,
            );
            this.currentStep.set('error');
          },
        });
    } catch (encryptionError: any) {
      this.validationError.set(encryptionError.message ?? 'Fehler bei der Verschlüsselung.');
      this.currentStep.set('error');
    }
  }

  computeAnother(): void {
    this.computationResult.set(null);
    this.currentStep.set('enter-list');
  }

  reset(): void {
    this.activeKeyPair = null;
    this.rawListInput.set('');
    this.computationResult.set(null);
    this.validationError.set('');
    this.currentStep.set('init');
  }

  /**
   * Parst eine Komma-getrennte Zeichenkette in eine Liste von Ganzzahlen.
   * Akzeptiert Werte im i32-Bereich [-2.147.483.648, 2.147.483.647] —
   * die tatsächlich verwendete Bitbreite wird separat per `selectOptimalBitWidth` bestimmt.
   * @throws Error bei nicht-ganzzahligen oder außerhalb des i32-Bereichs liegenden Werten
   */
  private parseAndValidateInput(rawInput: string): number[] {
    return rawInput
      .split(',')
      .map((token) => token.trim())
      .filter((token) => token.length > 0)
      .map((token) => {
        const parsedNumber = Number(token);
        if (!Number.isInteger(parsedNumber))
          throw new Error(`Kein gültiger ganzzahliger Wert: "${token}"`);
        if (parsedNumber < -2_147_483_648 || parsedNumber > 2_147_483_647)
          throw new Error(`Wert außerhalb des i32-Bereichs [-2.147.483.648, 2.147.483.647]: "${token}"`);
        return parsedNumber;
      });
  }

  private selectOptimalBitWidth(numbers: number[]): 8 | 16 | 32 {
    return selectOptimalBitWidth(numbers);
  }

  /** Verschlüsselt einen Klartextwert mit dem zum `bitWidth` passenden TFHE-Typ. */
  private encryptWithBitWidth(plainNumber: number, bitWidth: 8 | 16 | 32): Uint8Array {
    switch (bitWidth) {
      case 8:  return this.tfheService.encryptInt8(plainNumber,  this.activeKeyPair!.clientKey);
      case 16: return this.tfheService.encryptInt16(plainNumber, this.activeKeyPair!.clientKey);
      case 32: return this.tfheService.encryptInt32(plainNumber, this.activeKeyPair!.clientKey);
    }
  }

  /**
   * Entschlüsselt alle Felder der Server-Antwort passend zur verwendeten Bitbreite.
   * Summe und Durchschnitt liegen eine Stufe breiter vor (Overflow-Schutz):
   *   Int8-Eingabe  → Int16-Summe/Avg
   *   Int16-Eingabe → Int32-Summe/Avg
   *   Int32-Eingabe → Int64-Summe/Avg (als bigint; Number()-Cast sicher, da realistische
   *                                    Eingaben weit unter Number.MAX_SAFE_INTEGER = 2^53 liegen)
   */
  private decryptStatisticsResponse(
    encryptedResponse: ReturnType<StatisticsApiService['compute']> extends import('rxjs').Observable<infer R> ? R : never,
    bitWidth: 8 | 16 | 32,
  ): StatisticsResult {
    const fromBase64 = (b64: string) => this.tfheService.fromBase64(b64);
    const clientKey  = this.activeKeyPair!.clientKey;

    switch (bitWidth) {
      case 8:
        return {
          sum:      this.tfheService.decryptInt16(fromBase64(encryptedResponse.sum),     clientKey),
          count:    encryptedResponse.count,
          min:      this.tfheService.decryptInt8( fromBase64(encryptedResponse.min),     clientKey),
          max:      this.tfheService.decryptInt8( fromBase64(encryptedResponse.max),     clientKey),
          average:  this.tfheService.decryptInt16(fromBase64(encryptedResponse.average), clientKey),
          median:   this.tfheService.decryptInt8( fromBase64(encryptedResponse.median),  clientKey),
          bitWidth: encryptedResponse.bit_width,
        };
      case 16:
        return {
          sum:      this.tfheService.decryptInt32(fromBase64(encryptedResponse.sum),     clientKey),
          count:    encryptedResponse.count,
          min:      this.tfheService.decryptInt16(fromBase64(encryptedResponse.min),     clientKey),
          max:      this.tfheService.decryptInt16(fromBase64(encryptedResponse.max),     clientKey),
          average:  this.tfheService.decryptInt32(fromBase64(encryptedResponse.average), clientKey),
          median:   this.tfheService.decryptInt16(fromBase64(encryptedResponse.median),  clientKey),
          bitWidth: encryptedResponse.bit_width,
        };
      case 32:
        return {
          // Int64 dekryptiert als bigint; Number()-Cast hier sicher:
          // Selbst 1.000 × i32::MAX ≈ 2 Billionen liegt weit unter Number.MAX_SAFE_INTEGER (2^53 ≈ 9 Billiarden).
          sum:      Number(this.tfheService.decryptInt64(fromBase64(encryptedResponse.sum),     clientKey)),
          count:    encryptedResponse.count,
          min:      this.tfheService.decryptInt32(fromBase64(encryptedResponse.min),     clientKey),
          max:      this.tfheService.decryptInt32(fromBase64(encryptedResponse.max),     clientKey),
          average:  Number(this.tfheService.decryptInt64(fromBase64(encryptedResponse.average), clientKey)),
          median:   this.tfheService.decryptInt32(fromBase64(encryptedResponse.median),  clientKey),
          bitWidth: encryptedResponse.bit_width,
        };
    }
  }
}

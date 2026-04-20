import { Component, signal } from '@angular/core';
import { TfheService } from '../../core/crypto/tfhe.service';
import { AgeVerificationApiService } from '../../core/api/age-verification-api.service';
import { KeyPair } from '../../core/crypto/key-pair.model';
import { ButtonComponent } from '../../shared/components/button/button.component';
import { InputComponent } from '../../shared/components/input/input.component';
import { SpinnerComponent } from '../../shared/components/spinner/spinner.component';

type Step = 'init' | 'generating' | 'enter-age' | 'verifying' | 'result' | 'error';

@Component({
  selector: 'app-age-verification',
  imports: [ButtonComponent, InputComponent, SpinnerComponent],
  templateUrl: './age-verification.component.html',
  styleUrl: './age-verification.component.css',
})
export class AgeVerificationComponent {
  step = signal<Step>('init');
  ageInput = signal('');
  isAdult = signal<boolean | null>(null);
  errorMessage = signal('');

  private keyPair: KeyPair | null = null;

  constructor(
    private tfhe: TfheService,
    private api: AgeVerificationApiService
  ) {}

  async generateKeys(): Promise<void> {
    this.step.set('generating');
    // Yield to allow Angular to render the loading state before blocking
    await new Promise((resolve) => setTimeout(resolve, 50));

    try {
      await this.tfhe.ensureInitialized();
      this.keyPair = this.tfhe.generateKeyPair();
      this.step.set('enter-age');
    } catch (e) {
      this.errorMessage.set('Fehler beim Generieren der Schlüssel.');
      this.step.set('error');
    }
  }

  async verify(): Promise<void> {
    const ageValue = parseInt(this.ageInput(), 10);
    if (isNaN(ageValue) || ageValue < 0 || ageValue > 255) {
      this.errorMessage.set('Bitte ein gültiges Alter (0–255) eingeben.');
      this.step.set('error');
      return;
    }
    if (!this.keyPair) return;

    this.step.set('verifying');
    await new Promise((resolve) => setTimeout(resolve, 50));

    try {
      const encryptedAgeBytes = this.tfhe.encryptUint8(ageValue, this.keyPair.clientKey);
      const encryptedAgeB64 = this.tfhe.toBase64(encryptedAgeBytes);
      const serverKeyB64 = this.tfhe.toBase64(this.keyPair.serverKeyBytes);

      this.api.verify(encryptedAgeB64, serverKeyB64).subscribe({
        next: (isAdultB64) => {
          const isAdultBytes = this.tfhe.fromBase64(isAdultB64);
          const result = this.tfhe.decryptBool(isAdultBytes, this.keyPair!.clientKey);
          this.isAdult.set(result);
          this.step.set('result');
        },
        error: (err) => {
          this.errorMessage.set(`Server-Fehler: ${err.message ?? 'Unbekannter Fehler'}`);
          this.step.set('error');
        },
      });
    } catch (e) {
      this.errorMessage.set('Fehler bei der Verschlüsselung.');
      this.step.set('error');
    }
  }

  verifyAnother(): void {
    this.ageInput.set('');
    this.isAdult.set(null);
    this.step.set('enter-age');
  }

  reset(): void {
    this.keyPair = null;
    this.ageInput.set('');
    this.isAdult.set(null);
    this.errorMessage.set('');
    this.step.set('init');
  }
}

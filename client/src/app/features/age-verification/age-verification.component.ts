import { Component, signal, OnDestroy } from '@angular/core';
import { TfheService } from '../../core/crypto/tfhe.service';
import { AgeVerificationApiService } from '../../core/api/age-verification-api.service';
import { KeyPair } from '../../core/crypto/key-pair.model';
import { ButtonComponent } from '../../shared/components/button/button.component';
import { InputComponent } from '../../shared/components/input/input.component';
import { SpinnerComponent } from '../../shared/components/spinner/spinner.component';

type Step = 'init' | 'generating' | 'uploading' | 'enter-age' | 'verifying' | 'result' | 'error';

@Component({
  selector: 'app-age-verification',
  imports: [ButtonComponent, InputComponent, SpinnerComponent],
  templateUrl: './age-verification.component.html',
  styleUrl: './age-verification.component.css',
})
export class AgeVerificationComponent implements OnDestroy {
  step = signal<Step>('init');
  ageInput = signal('');
  isAdult = signal<boolean | null>(null);
  errorMessage = signal('');
  verifyDuration = signal<number | null>(null);

  private keyPair: KeyPair | null = null;
  private sessionId: string | null = null;

  constructor(
    private tfhe: TfheService,
    private api: AgeVerificationApiService,
  ) {}

  /**
   * Phase 1: Schlüsselpaar generieren + ServerKey einmalig hochladen.
   * Nach erfolgreichem Setup ist die session_id gecacht und alle
   * folgenden Verify-Requests senden nur noch encrypted_ag.
   */
  async generateKeys(): Promise<void> {
    this.step.set('generating');
    await new Promise((resolve) => setTimeout(resolve, 50));

    try {
      await this.tfhe.ensureInitialized();
      this.keyPair = this.tfhe.generateKeyPair();

      // ServerKey einmalig hochladen
      this.step.set('uploading');
      await new Promise((resolve) => setTimeout(resolve, 50));

      const serverKeyB64 = this.tfhe.toBase64(this.keyPair.serverKeyBytes);
      this.api.setupSession(serverKeyB64).subscribe({
        next: (sessionId) => {
          this.sessionId = sessionId;
          this.step.set('enter-age');
        },
        error: (err) => {
          this.errorMessage.set(`Fehler beim Session-Setup: ${err.message ?? 'Unbekannter Fehler'}`);
          this.step.set('error');
        },
      });
    } catch (e) {
      this.errorMessage.set('Fehler beim Generieren der Schlüssel.');
      this.step.set('error');
    }
  }

  /**
   * Phase 2: Nur encrypted_age an den Server senden.
   * Der ServerKey liegt bereits gecacht auf dem Server.
   */
  async verify(): Promise<void> {
    const ageValue = parseInt(this.ageInput(), 10);
    if (isNaN(ageValue) || ageValue < 0 || ageValue > 127) {
      this.errorMessage.set('Bitte ein gültiges Alter (0-127) eingeben.');
      this.step.set('error');
      return;
    }
    if (!this.keyPair || !this.sessionId) return;

    this.step.set('verifying');
    await new Promise((resolve) => setTimeout(resolve, 50));

    try {
      const encryptedAgeBytes = this.tfhe.encryptUint8(ageValue, this.keyPair.clientKey);
      const encryptedAgeB64 = this.tfhe.toBase64(encryptedAgeBytes);
      const start = Date.now();

      this.api.verify(this.sessionId, encryptedAgeB64).subscribe({
        next: (isAdultB64) => {
          this.verifyDuration.set(Math.round((Date.now() - start) / 10) / 100);
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

  /**
   * Weitere Verifikation mit demselben Schlüsselpaar und derselben Session.
   * Kein neuer ServerKey-Upload nötig.
   */
  verifyAnother(): void {
    this.ageInput.set('');
    this.isAdult.set(null);
    this.step.set('enter-age');
  }

  /**
   * Vollständiger Reset: Session löschen + lokalen State zurücksetzen.
   */
  reset(): void {
    if (this.sessionId) {
      this.api.deleteSession(this.sessionId).subscribe();
    }
    this.sessionId = null;
    this.keyPair = null;
    this.ageInput.set('');
    this.isAdult.set(null);
    this.errorMessage.set('');
    this.step.set('init');
  }

  /**
   * Session beim Verlassen der Komponente aufräumen.
   */
  ngOnDestroy(): void {
    if (this.sessionId) {
      this.api.deleteSession(this.sessionId).subscribe();
    }
  }
}
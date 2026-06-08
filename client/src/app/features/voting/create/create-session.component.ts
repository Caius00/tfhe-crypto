import { Component, computed, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { CommonModule } from '@angular/common';
import { TfheService } from '../../../core/crypto/tfhe.service';
import { VotingService } from '../voting.service';
import { Question } from '../voting.types';

import { PageHeaderComponent } from '../../../shared/components/page-header/page-header.component';
import { CardComponent } from '../../../shared/components/card/card.component';
import { InputComponent } from '../../../shared/components/input/input.component';
import { ButtonComponent } from '../../../shared/components/button/button.component';
import { AlertComponent } from '../../../shared/components/alert/alert.component';
import { LoadingOverlayComponent } from '../../../shared/components/loading-overlay/loading-overlay.component';
import { BadgeComponent } from '../../../shared/components/badge/badge.component';
import { QuestionEditorComponent } from '../components/question-editor/question-editor.component';

/**
 * Maske zum Erstellen einer neuen Voting-Session.
 *
 * Ablauf:
 *   1) Creator-ID eintragen
 *   2) FHE-Schlüsselpaar generieren (blockierend, Loading-Overlay)
 *   3) Fragen anlegen (über QuestionEditor-Subkomponente)
 *   4) Session am Backend erstellen → Redirect auf /voting/manage/:id
 */
@Component({
  selector: 'app-create-session',
  standalone: true,
  imports: [
    CommonModule,
    PageHeaderComponent,
    CardComponent,
    InputComponent,
    ButtonComponent,
    AlertComponent,
    LoadingOverlayComponent,
    BadgeComponent,
    QuestionEditorComponent,
  ],
  templateUrl: './create-session.component.html',
  styleUrl: './create-session.component.css',
})
export class CreateSessionComponent {
  private votingService = inject(VotingService);
  private tfhe = inject(TfheService);
  private router = inject(Router);

  // --- Reaktiver State (Angular Signals) ------------------------------------

  /** Eingegebene Creator-ID (Identifikation gegenüber Backend) */
  creatorId = signal('');
  /** Liste der Fragen (mind. eine, leere Initialfrage) */
  questions = signal<Question[]>([
    { id: 1, text: '', question_type: 'single', options: null},
  ]);

  /** Generiertes FHE-KeyPair – null bis "Keys generieren" geklickt wurde */
  private keyPair: { clientKey: any; serverKeyBytes: Uint8Array; publicKeyBytes?: Uint8Array } | null = null;

  /** Ist gerade die Schlüsselgenerierung aktiv? (blockiert UI) */
  isGeneratingKeys = signal(false);
  /** Ist gerade der Create-Session-Request aktiv? */
  isCreating = signal(false);
  /** Sind Schlüssel bereits vorhanden? */
  hasKeys = signal(false);
  /** Fehlermeldung (für AlertComponent) */
  errorMessage = signal<string | null>(null);
  /** Erfolgsmeldung */
  successMessage = signal<string | null>(null);

  /** Validierung: ist das Formular bereit zum Absenden? */
  canSubmit = computed(() => {
    if (!this.hasKeys() || !this.creatorId().trim()) return false;
    const qs = this.questions();
    if (qs.length === 0) return false;
    // Jede Frage muss Text haben; bei Single/Multiple: mind. 2 nicht-leere Optionen
    return qs.every((q) => {
      if (!q.text.trim()) return false;
      if (q.question_type === 'single' || q.question_type === 'multiple') {
        const opts = (q.options ?? []).filter((o) => o.trim());
        return opts.length >= 2;
      }
      return true;
    });
  });

  // --- Schlüssel ------------------------------------------------------------

  /**
   * Erzeugt das FHE-Schlüsselpaar (Client-Key + Server-Key + Public-Key).
   * Der Aufruf ist blockierend (30–90s) – wir setzen ein kleines setTimeout
   * damit der Loading-Overlay vor dem Block gerendert werden kann.
   */
  async generateKeys(): Promise<void> {
    this.errorMessage.set(null);
    this.isGeneratingKeys.set(true);

    // Kleiner Yield damit Angular das Overlay rendert bevor WASM blockiert
    await new Promise((r) => setTimeout(r, 50));

    try {
      await this.tfhe.ensureInitialized();
      const kp = this.tfhe.generateKeyPair();
      // CompactPublicKey statt CompressedPublicKey – ermöglicht Batch-Verschlüsselung
      // beim Join (Name) und Vote (alle Antwort-Bits in einem Schlag).
      const publicKeyBytes = this.tfhe.generatePublicKey(kp.clientKey);
      this.keyPair = { ...kp, publicKeyBytes };
      this.hasKeys.set(true);
      this.successMessage.set('Schlüssel erfolgreich erzeugt.');
    } catch (e) {
      console.error('Key generation failed', e);
      this.errorMessage.set('Schlüsselgenerierung fehlgeschlagen: ' + (e as Error).message);
    } finally {
      this.isGeneratingKeys.set(false);
    }
  }
  // --- Fragen-Management ----------------------------------------------------

  /** Update einer Frage (von QuestionEditor emittiert) */
  updateQuestion(idx: number, q: Question): void {
    this.questions.update((arr) => arr.map((old, i) => (i === idx ? q : old)));
  }

  /** Neue leere Frage hinzufügen */
  addQuestion(): void {
    this.questions.update((arr) => [
      ...arr,
      { id: arr.length + 1, text: '', question_type: 'single', options: null},
    ]);
  }

  /** Frage entfernen (mind. eine bleibt erhalten) */
  removeQuestion(idx: number): void {
    this.questions.update((arr) => (arr.length <= 1 ? arr : arr.filter((_, i) => i !== idx)));
  }

  // --- Session erstellen ----------------------------------------------------

  create(): void {
    if (!this.canSubmit() || !this.keyPair) return;

    this.errorMessage.set(null);
    this.successMessage.set(null);
    this.isCreating.set(true);
    //console.log("Server Key Bytes: " + this.keyPair.serverKeyBytes);
    const serverKeyB64 = this.tfhe.toBase64(this.keyPair.serverKeyBytes);
    //console.log("Server Key Base64: " + serverKeyB64);
    //console.log("Public Key Bytes: " + this.keyPair.publicKeyBytes);
    const publicKeyB64 = this.keyPair.publicKeyBytes
      ? this.tfhe.toBase64(this.keyPair.publicKeyBytes)
      : null;
    //console.log("Public Key Base64: " + publicKeyB64);

    // Bereinigte Fragen ans Backend (leere Optionen rauswerfen)
    const cleanedQuestions: Question[] = this.questions().map((q) => ({
      ...q,
      options: q.options ? q.options.filter((o) => o.trim()) : null,
    }));

    this.votingService
      .createSession(this.creatorId().trim(), serverKeyB64, publicKeyB64, cleanedQuestions)
      .subscribe({
        next: (res) => {
          // Creator-ID persistieren (zwischen Sessions wiederverwendbar)
          localStorage.setItem('creatorId', this.creatorId().trim());

          // Schlüssel pro Session ablegen, damit /manage sie anzeigen kann.
          // Wichtig: Server-Key wird NICHT gespeichert. Er ist mehrere MB groß
          // und sprengt das sessionStorage-Quota (typ. 5 MB pro Origin) –
          // das hat vorher zu QuotaExceededError und endlosem Loading geführt.
          // In der UI wird er ohnehin nicht gebraucht (er liegt am Server).
          const clientKeyBytes = this.keyPair!.clientKey.serialize();
          this.safeSessionSet(`clientKey_${res.session_id}`, this.tfhe.toBase64(clientKeyBytes));
          if (publicKeyB64) {
            this.safeSessionSet(`publicKey_${res.session_id}`, publicKeyB64);
          }

          this.successMessage.set('Session erstellt. Wechsle zur Verwaltung...');

          // Loading-State zurücksetzen falls Navigation aus irgendeinem Grund
          // nicht greift (defensiv – sonst hängt der Button "loading"
          // auf einer kaputten Route).
          this.isCreating.set(false);

          this.router.navigateByUrl(`/voting/manage/${res.session_id}`);
        },
        error: (err) => {
          console.error('Create session failed', err);
          this.errorMessage.set(
            'Session konnte nicht erstellt werden. Bitte später erneut versuchen.',
          );
          this.isCreating.set(false);
        },
      });
  }

  /**
   * Schreibt einen Wert in sessionStorage. Fängt QuotaExceededError ab –
   * passiert wenn ein Wert (z.B. Public-Key) zu groß ist. In dem Fall
   * loggen wir nur, das Feature "Schlüssel anzeigen" zeigt den Eintrag
   * dann nicht – die Session-Erstellung selbst soll niemals daran scheitern.
   */
  private safeSessionSet(key: string, value: string): void {
    try {
      sessionStorage.setItem(key, value);
    } catch (e) {
      console.warn(`sessionStorage konnte ${key} nicht speichern (Quota?)`, e);
    }
  }
}

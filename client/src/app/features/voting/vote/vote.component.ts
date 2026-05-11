import { Component, OnInit, computed, inject, signal } from '@angular/core';
import { ActivatedRoute, Router } from '@angular/router';
import { CommonModule } from '@angular/common';
import { firstValueFrom } from 'rxjs';

import { TfheService } from '../../../core/crypto/tfhe.service';
import { VotingService } from '../voting.service';
import { Question } from '../voting.types';

import { PageHeaderComponent } from '../../../shared/components/page-header/page-header.component';
import { ButtonComponent } from '../../../shared/components/button/button.component';
import { AlertComponent } from '../../../shared/components/alert/alert.component';
import { LoadingOverlayComponent } from '../../../shared/components/loading-overlay/loading-overlay.component';
import { CardComponent } from '../../../shared/components/card/card.component';

import {
  AnswerValue,
  QuestionInputComponent,
} from '../components/question-input/question-input.component';

/**
 * Abstimmungs-Seite für freigegebene Teilnehmer.
 *
 * Lädt Fragen + Public-Key, sammelt Antworten in `answers()` und verschlüsselt
 * beim Submit jede Antwort homomorph mit dem Public-Key der Session.
 *
 * Encoding-Regeln (übereinstimmend mit dem Backend):
 *   - bool      → [enc(0|1)]
 *   - single    → One-Hot (Array mit 0/1, eine 1 an Index der Auswahl)
 *   - multiple  → Multi-Hot (Array mit 0/1 pro Option)
 *   - numeric   → [enc(value)]
 */
@Component({
  selector: 'app-vote',
  standalone: true,
  imports: [
    CommonModule,
    PageHeaderComponent,
    ButtonComponent,
    AlertComponent,
    LoadingOverlayComponent,
    CardComponent,
    QuestionInputComponent,
  ],
  templateUrl: './vote.component.html',
  styleUrl: './vote.component.css',
})
export class VoteComponent implements OnInit {
  private route = inject(ActivatedRoute);
  private votingService = inject(VotingService);
  private tfhe = inject(TfheService);
  private router = inject(Router);

  // --- State ----------------------------------------------------------------

  questions = signal<Question[]>([]);
  /** Antworten in derselben Reihenfolge wie questions() */
  answers = signal<AnswerValue[]>([]);

  isLoading = signal(true);
  isSubmitting = signal(false);
  hasSubmitted = signal(false);
  errorMessage = signal<string | null>(null);
  successMessage = signal<string | null>(null);

  /** Sind alle Fragen beantwortet? Verhindert "leere" Stimmen */
  canSubmit = computed(() => {
    if (this.isSubmitting() || this.hasSubmitted()) return false;
    const qs = this.questions();
    const ans = this.answers();
    if (qs.length === 0) return false;

    return qs.every((q, i) => {
      const v = ans[i];
      switch (q.question_type) {
        case 'bool':     return typeof v === 'boolean';
        case 'numeric':  return typeof v === 'number' && Number.isFinite(v);
        case 'single':   return typeof v === 'number';
        case 'multiple': return Array.isArray(v) && v.length > 0;
      }
    });
  });

  private sessionId = '';
  private publicKeyB64: string | null = null;

  // --- Lifecycle ------------------------------------------------------------

  async ngOnInit(): Promise<void> {
    this.sessionId = this.route.snapshot.paramMap.get('sessionId') ?? '';
    if (!this.sessionId) {
      this.errorMessage.set('Keine Session-ID in der URL.');
      this.isLoading.set(false);
      return;
    }

    try {
      await this.tfhe.ensureInitialized();

      // Public-Key per-Session cachen damit verschiedene Sessions sich nicht in die Quere kommen
      const cacheKey = `tfhe_public_key_${this.sessionId}`;
      this.publicKeyB64 = sessionStorage.getItem(cacheKey);

      const session = await firstValueFrom(this.votingService.getSession(this.sessionId));
      this.questions.set(session.questions ?? []);
      this.answers.set(new Array(this.questions().length).fill(undefined));

      // Falls Cache leer war: Public-Key aus Session-Antwort übernehmen.
      // Cache-Schreiben ist defensiv – Quota darf den Vote-Flow nicht killen.
      if (!this.publicKeyB64 && session.public_key) {
        this.publicKeyB64 = session.public_key;
        try {
          sessionStorage.setItem(cacheKey, session.public_key);
        } catch (e) {
          console.warn('Public-Key konnte nicht gecached werden', e);
        }
      }
      if (!this.publicKeyB64) {
        this.errorMessage.set('Public-Key der Session konnte nicht geladen werden.');
      }
    } catch (e) {
      console.error('Vote init failed', e);
      this.errorMessage.set('Fragen konnten nicht geladen werden.');
    } finally {
      this.isLoading.set(false);
    }
  }

  // --- Antworten ändern -----------------------------------------------------

  updateAnswer(idx: number, value: AnswerValue): void {
    this.answers.update((arr) => arr.map((old, i) => (i === idx ? value : old)));
  }

  // --- Submit ---------------------------------------------------------------

  async submit(): Promise<void> {
    if (!this.canSubmit() || !this.publicKeyB64) return;

    const participantId = localStorage.getItem('participantId');
    if (!participantId) {
      this.errorMessage.set('Keine Teilnehmer-ID gefunden.');
      return;
    }

    this.errorMessage.set(null);
    this.successMessage.set(null);
    this.isSubmitting.set(true);

    // Yield damit Loading-Overlay vor der (synchronen) Verschlüsselung sichtbar wird
    await new Promise((r) => setTimeout(r, 50));

    try {
      const encryptedVotes = this.encryptAnswers();
      await firstValueFrom(
        this.votingService.submitVote(this.sessionId, participantId, encryptedVotes),
      );
      this.successMessage.set('Stimme erfolgreich abgegeben. Vielen Dank!');
      this.hasSubmitted.set(true);
    } catch (e) {
      console.error('Submit vote failed', e);
      this.errorMessage.set('Stimme konnte nicht gesendet werden.');
    } finally {
      this.isSubmitting.set(false);
    }
  }

  /**
   * Verschlüsselt alle gegebenen Antworten zur Vote-Payload-Form für das Backend.
   *
   * Performance: Statt pro Frage und pro Option einzeln zu verschlüsseln
   * (= N FHE-Operationen), sammeln wir ALLE Werte in einer flachen Liste,
   * verschlüsseln sie in EINEM Batch (CompactCiphertextList) und splitten
   * danach zurück in die "Vote-Matrix" für das Backend.
   *
   * Beispiel: 3 Fragen mit je 4 Optionen (single+multiple) = 12 Werte →
   * eine einzige Krypto-Operation statt 12.
   *
   * Encoding pro Frage:
   *   - bool / numeric: 1 Wert
   *   - single:         N Werte (One-Hot über alle Optionen)
   *   - multiple:       N Werte (Multi-Hot über alle Optionen)
   */
  private encryptAnswers(): string[][] {
    const pk = this.publicKeyB64!;
    const qs = this.questions();
    const ans = this.answers();

    // 1) Flache Liste aller Werte aufbauen + Layout merken (wieviele Werte pro Frage)
    const flatValues: number[] = [];
    const valuesPerQuestion: number[] = [];

    for (let i = 0; i < qs.length; i++) {
      const q = qs[i];
      const v = ans[i];

      if (q.question_type === 'bool') {
        flatValues.push(v === true ? 1 : 0);
        valuesPerQuestion.push(1);
      } else if (q.question_type === 'numeric') {
        const num = typeof v === 'number' ? Math.max(0, Math.min(255, Math.round(v))) : 0;
        flatValues.push(num);
        valuesPerQuestion.push(1);
      } else if (q.question_type === 'single') {
        const selected = typeof v === 'number' ? v : -1;
        const opts = q.options ?? [];
        for (let idx = 0; idx < opts.length; idx++) {
          flatValues.push(idx === selected ? 1 : 0);
        }
        valuesPerQuestion.push(opts.length);
      } else if (q.question_type === 'multiple') {
        const selectedSet = new Set(Array.isArray(v) ? v : []);
        const opts = q.options ?? [];
        for (let idx = 0; idx < opts.length; idx++) {
          flatValues.push(selectedSet.has(idx) ? 1 : 0);
        }
        valuesPerQuestion.push(opts.length);
      } else {
        valuesPerQuestion.push(0);
      }
    }

    // 2) Eine einzige Krypto-Operation für ALLE Werte
    const encryptedFlat = this.tfhe.encryptUint8sCompact(pk, flatValues);

    // 3) Zurück in die Form "ein Array pro Frage" splitten
    const result: string[][] = [];
    let cursor = 0;
    for (const count of valuesPerQuestion) {
      result.push(encryptedFlat.slice(cursor, cursor + count));
      cursor += count;
    }
    return result;
  }

  goHome(): void {
    this.router.navigate(['/voting']);
  }
}

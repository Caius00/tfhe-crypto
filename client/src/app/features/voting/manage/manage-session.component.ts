import { Component, OnDestroy, OnInit, computed, inject, signal } from '@angular/core';
import { ActivatedRoute, Router } from '@angular/router';
import { CommonModule } from '@angular/common';
import { firstValueFrom } from 'rxjs';

import { VotingService } from '../voting.service';
import { TfheService } from '../../../core/crypto/tfhe.service';
import { Question } from '../voting.types';

import { PageHeaderComponent } from '../../../shared/components/page-header/page-header.component';
import { CardComponent } from '../../../shared/components/card/card.component';
import { ButtonComponent } from '../../../shared/components/button/button.component';
import { AlertComponent } from '../../../shared/components/alert/alert.component';
import { BadgeComponent } from '../../../shared/components/badge/badge.component';
import { LoadingOverlayComponent } from '../../../shared/components/loading-overlay/loading-overlay.component';

import {
  PendingEntryView,
  PendingListComponent,
} from '../components/pending-list/pending-list.component';
import {
  DecryptedResult,
  ResultsViewComponent,
} from '../components/results-view/results-view.component';
import { CopyButtonComponent } from '../../../shared/components/copy-button/copy-button.component';
import {
  KeyDisplayComponent,
  KeyEntry,
} from '../components/key-display/key-display.component';

const POLL_INTERVAL_MS = 2000;

/**
 * Verwaltungs-Seite für den Session-Ersteller.
 *
 * Zeigt:
 *   - Session-ID & Status
 *   - Pending-Liste mit Polling (alle 2s)
 *   - Ergebnis-Bereich (laden + entschlüsseln)
 *   - Aktionen: Session beenden
 */
@Component({
  selector: 'app-manage-session',
  standalone: true,
  imports: [
    CommonModule,
    PageHeaderComponent,
    CardComponent,
    ButtonComponent,
    AlertComponent,
    BadgeComponent,
    LoadingOverlayComponent,
    PendingListComponent,
    ResultsViewComponent,
    CopyButtonComponent,
    KeyDisplayComponent,
  ],
  templateUrl: './manage-session.component.html',
  styleUrl: './manage-session.component.css',
})
export class ManageSessionComponent implements OnInit, OnDestroy {
  private route = inject(ActivatedRoute);
  private votingService = inject(VotingService);
  private tfhe = inject(TfheService);
  private router = inject(Router);

  // --- State ----------------------------------------------------------------

  /** Session-ID aus der Route */
  sessionId = '';
  /** Creator-ID aus localStorage (vom Create-Schritt persistiert) */
  creatorId = '';

  /** Fragen der Session */
  questions = signal<Question[]>([]);
  /** Aktuell wartende Teilnehmer (View-Modell mit ggf. entschlüsseltem Namen) */
  pending = signal<PendingEntryView[]>([]);
  /** Verschlüsselte Roh-Ergebnisse vom Server (Base64) */
  encryptedResults = signal<string[][]>([]);
  /** Entschlüsselte Ergebnisse für die Ansicht */
  decryptedResults = signal<DecryptedResult[]>([]);

  /** Lade-/Status-Flags */
  isInitializing = signal(true);
  isLoadingResults = signal(false);
  isFinalizing = signal(false);
  isResultsReady = signal(false);

  /** Fehler / Hinweise */
  errorMessage = signal<string | null>(null);
  infoMessage = signal<string | null>(null);

  /** Anzeige des Status als Badge */
  statusLabel = computed(() => {
    if (this.isInitializing()) return 'Lädt...';
    if (this.isResultsReady()) return 'Auswertung verfügbar';
    return 'Aktiv';
  });
  statusVariant = computed<'success' | 'info' | 'warning'>(() => {
    if (this.isResultsReady()) return 'success';
    if (this.isInitializing()) return 'warning';
    return 'info';
  });

  /** Schlüssel der Session zur Anzeige im Key-Display */
  sessionKeys = signal<KeyEntry[]>([]);
  /** Soll der Schlüssel-Bereich aufgeklappt sein? */
  showKeys = signal(false);

  /** Lokale Status-Meldung speziell für den Auswertungs-Bereich. */
  resultsStatus = signal<{ kind: 'info' | 'error' | 'success'; message: string } | null>(null);

  /** Privater Client-Key für Entschlüsselung – wird in ngOnInit geladen */
  private clientKey: any | null = null;
  /** Polling-Timer für Pending-Anfragen */
  private pollTimer: any = null;

  // --- Lifecycle ------------------------------------------------------------

  async ngOnInit(): Promise<void> {
    this.sessionId = this.route.snapshot.paramMap.get('sessionId') ?? '';
    this.creatorId = localStorage.getItem('creatorId') ?? '';

    if (!this.sessionId) {
      this.errorMessage.set('Keine Session-ID in der URL gefunden.');
      this.isInitializing.set(false);
      return;
    }
    if (!this.creatorId) {
      this.errorMessage.set('Keine Creator-ID gespeichert. Bitte Session neu erstellen.');
      this.isInitializing.set(false);
      return;
    }

    // WICHTIG: Polling für Pending-Anfragen sofort starten, unabhängig vom
    // Rest der Initialisierung. Die Pending-Liste hängt nur von session_id
    // und creator_id ab – nicht von Fragen, WASM oder Client-Key.
    // Vorher hat ein Fehler in loadSession() das Polling komplett verhindert.
    this.collectSessionKeys();
    this.startPolling();

    // WASM-Init, Fragen und Client-Key werden unabhängig voneinander geladen.
    // Fehler in einem dieser Schritte blockieren nicht die anderen oder das Polling.
    try {
      await this.tfhe.ensureInitialized();
    } catch (e) {
      console.error('TFHE WASM init failed', e);
    }

    try {
      await this.loadSession();
    } catch (e) {
      console.warn('Fragen konnten nicht geladen werden', e);
      this.infoMessage.set(
        'Hinweis: Fragen konnten nicht geladen werden. ' +
        'Anfragen-Verwaltung funktioniert trotzdem.',
      );
    }

    try {
      await this.loadClientKey();
    } catch (e) {
      console.warn('Client-Key fehlt – Namen können nicht entschlüsselt werden', e);
      this.infoMessage.set(
        'Hinweis: Kein Client-Key in diesem Tab gefunden. ' +
        'Anfragen können freigegeben/abgelehnt werden, Namen bleiben aber verschlüsselt. ' +
        'Ergebnisse können in diesem Tab nicht entschlüsselt werden.',
      );
    }

    this.isInitializing.set(false);
  }

  ngOnDestroy(): void {
    this.stopPolling();
  }

  // --- Initial-Loading ------------------------------------------------------

  /** Lädt Session-Metadaten (Fragen) vom Server */
  private async loadSession(): Promise<void> {
    try {
      const res = await firstValueFrom(this.votingService.getSession(this.sessionId));
      this.questions.set(res.questions ?? []);
    } catch (e) {
      console.error('Load session failed', e);
      throw new Error('Session konnte nicht geladen werden');
    }
  }

  /**
   * Sammelt alle Schlüssel der Session aus dem sessionStorage in das KeyEntry-Format.
   * Ist ein Schlüssel nicht im Storage (z.B. anderer Tab), wird er übersprungen.
   */
  private collectSessionKeys(): void {
    const entries: KeyEntry[] = [];

    const clientB64 = sessionStorage.getItem(`clientKey_${this.sessionId}`);
    if (clientB64) {
      entries.push({
        label: 'Client-Key (privat)',
        description:
          'Bleibt nur in deinem Browser. Wird zum Entschlüsseln von Namen und Ergebnissen verwendet. Niemals teilen!',
        value: clientB64,
      });
    }

    const publicB64 = sessionStorage.getItem(`publicKey_${this.sessionId}`);
    if (publicB64) {
      entries.push({
        label: 'Public-Key',
        description:
          'Öffentlicher Schlüssel. Teilnehmer verschlüsseln damit ihren Namen und ihre Stimmen.',
        value: publicB64,
      });
    }

    // Server-Key wird absichtlich nicht in sessionStorage gehalten –
    // er ist mehrere MB groß und sprengt die Browser-Quota (~5 MB).
    // Er liegt am Server und ist für die UI ohnehin uninteressant.

    this.sessionKeys.set(entries);
  }

  /** Lädt den Client-Key aus dem sessionStorage und deserialisiert ihn */
  private async loadClientKey(): Promise<void> {
    const stored = sessionStorage.getItem(`clientKey_${this.sessionId}`);
    if (!stored) {
      throw new Error('Kein Client-Key im Browser gefunden – bitte Session-Tab nicht geschlossen haben.');
    }
    try {
      const bytes = this.tfhe.fromBase64(stored);
      const { TfheClientKey } = await import('tfhe');
      this.clientKey = TfheClientKey.deserialize(bytes);
    } catch (e) {
      console.error('Client-Key Deserialisierung fehlgeschlagen', e);
      throw new Error('Client-Key konnte nicht geladen werden.');
    }
  }

  // --- Polling der Pending-Liste --------------------------------------------

  private startPolling(): void {
    this.fetchPending();
    this.pollTimer = setInterval(() => this.fetchPending(), POLL_INTERVAL_MS);
  }

  private stopPolling(): void {
    if (this.pollTimer) {
      clearInterval(this.pollTimer);
      this.pollTimer = null;
    }
  }

  private fetchPending(): void {
    this.votingService.getPending(this.sessionId, this.creatorId).subscribe({
      next: (res) => {
        // Bestehende Entschlüsselungen behalten, falls Teilnehmer noch in Liste ist
        const previous = new Map(
          this.pending().map((p) => [p.participantId, p.decryptedName] as const),
        );
        this.pending.set(
          res.map((entry) => ({
            participantId: entry.participant_id,
            encNameChunks: entry.enc_name_chunks ?? [],
            decryptedName: previous.get(entry.participant_id),
          })),
        );
        // Wenn Polling vorher Fehler hatte: nach Erfolg löschen
        if (this.errorMessage()?.startsWith('Pending-Anfragen')) {
          this.errorMessage.set(null);
        }
      },
      error: (err) => {
        console.error('Fetch pending failed', err);
        // Sichtbarer Hinweis statt stiller Konsolenfehler
        this.errorMessage.set(
          'Pending-Anfragen konnten nicht geladen werden. Verbindung zum Server prüfen.',
        );
      },
    });
  }

  // --- Pending-Aktionen -----------------------------------------------------

  /** Entschlüsselt den (verschlüsselten) Namen eines Teilnehmers in der Liste */
  decryptName(entry: PendingEntryView): void {
    if (!this.clientKey) {
      this.errorMessage.set('Kein Client-Key verfügbar.');
      return;
    }
    if (!entry.encNameChunks.length) {
      this.updatePendingName(entry.participantId, '(kein Name)');
      return;
    }

    try {
      const bytes: number[] = [];
      for (const chunkB64 of entry.encNameChunks) {
        const raw = this.tfhe.fromBase64(chunkB64);
        bytes.push(this.tfhe.decryptUint8(raw, this.clientKey));
      }
      const name = new TextDecoder().decode(new Uint8Array(bytes));
      this.updatePendingName(entry.participantId, name);
    } catch (e) {
      console.error('Decrypt name failed', e);
      this.updatePendingName(entry.participantId, '(Fehler beim Entschlüsseln)');
    }
  }

  /** Hilfsfunktion: aktualisiert den entschlüsselten Namen im Pending-State */
  private updatePendingName(participantId: string, name: string): void {
    this.pending.update((arr) =>
      arr.map((p) => (p.participantId === participantId ? { ...p, decryptedName: name } : p)),
    );
  }

  approve(participantId: string): void {
    this.votingService
      .approveParticipant(this.sessionId, this.creatorId, participantId, true)
      .subscribe({
        next: () => this.fetchPending(),
        error: () => this.errorMessage.set('Annahme fehlgeschlagen.'),
      });
  }

  reject(participantId: string): void {
    this.votingService
      .approveParticipant(this.sessionId, this.creatorId, participantId, false)
      .subscribe({
        next: () => this.fetchPending(),
        error: () => this.errorMessage.set('Ablehnung fehlgeschlagen.'),
      });
  }

  // --- Ergebnisse -----------------------------------------------------------

  /**
   * Lädt die Aggregate vom Server UND entschlüsselt sie direkt im Anschluss.
   *
   * Der Server liefert verschlüsselte Aggregate (homomorph aufsummiert) plus
   * ein `ready`-Flag. Ist `ready` false (noch nicht alle freigegebenen Teilnehmer
   * haben abgestimmt), wird die Entschlüsselung übersprungen und ein Hinweis
   * angezeigt. Sonst werden die Ciphertexts lokal mit dem Client-Key entschlüsselt.
   */
  showResults(): void {
    console.log('[Voting] showResults clicked', {
      sessionId: this.sessionId,
      creatorId: this.creatorId,
      hasClientKey: !!this.clientKey,
      questionCount: this.questions().length,
    });

    if (!this.clientKey) {
      this.resultsStatus.set({
        kind: 'error',
        message: 'Kein Client-Key in diesem Tab. Auswertung nur in dem Tab möglich, in dem die Session erstellt wurde.',
      });
      return;
    }

    this.resultsStatus.set({ kind: 'info', message: 'Lade Aggregate vom Server...' });
    this.isLoadingResults.set(true);
    this.decryptedResults.set([]);

    this.votingService.getResults(this.sessionId, this.creatorId).subscribe({
      next: (res) => {
        console.log('[Voting] /results response', {
          ready: res.ready,
          resultGroups: res.encrypted_results?.length ?? 0,
        });

        this.encryptedResults.set(res.encrypted_results as unknown as string[][]);
        this.isResultsReady.set(res.ready);

        if (!res.ready) {
          this.resultsStatus.set({
            kind: 'info',
            message:
              'Ergebnisse noch nicht vollständig – es haben noch nicht alle freigegebenen Teilnehmer abgestimmt. ' +
              'Pending-Liste oben zeigt noch wartende Anfragen; ein Klick auf "Ergebnisse laden" wenn alle abgestimmt haben.',
          });
          this.isLoadingResults.set(false);
          return;
        }

        // Edge-Case: Server sagt ready, schickt aber leere Aggregate
        const enc = res.encrypted_results as unknown as string[][];
        if (!enc || enc.length === 0) {
          this.resultsStatus.set({
            kind: 'error',
            message: 'Server meldet "ready" aber keine Aggregate. Vermutlich ein Server-Fehler – siehe Server-Logs.',
          });
          this.isLoadingResults.set(false);
          return;
        }

        // Server-Aggregate liegen vor → lokal entschlüsseln
        try {
          const decrypted = this.decryptAggregates(enc);
          this.decryptedResults.set(decrypted);
          this.resultsStatus.set({
            kind: 'success',
            message: `Auswertung erfolgreich entschlüsselt (${decrypted.length} Frage${decrypted.length === 1 ? '' : 'n'}).`,
          });
        } catch (e) {
          console.error('Decrypt aggregates failed', e);
          this.resultsStatus.set({
            kind: 'error',
            message: 'Entschlüsselung der Ergebnisse fehlgeschlagen: ' + (e as Error).message,
          });
        } finally {
          this.isLoadingResults.set(false);
        }
      },
      error: (err) => {
        console.error('Load results failed', err);
        // Backend-Fehlertext durchreichen falls vorhanden – hilft bei Diagnose.
        const detail = err?.error || err?.message || 'unbekannt';
        this.resultsStatus.set({
          kind: 'error',
          message: `Ergebnisse konnten nicht geladen werden: ${typeof detail === 'string' ? detail : JSON.stringify(detail)}`,
        });
        this.isLoadingResults.set(false);
      },
    });
  }

  /**
   * Mappt verschlüsselte Aggregate auf lesbare Ergebnisse (pro Frage).
   * Für Bool/Numeric: skalar. Für Single/Multiple: pro Option ein Zähler.
   */
  private decryptAggregates(encrypted: string[][]): DecryptedResult[] {
    const qs = this.questions();
    return encrypted.map((entry, qIdx) => {
      const q = qs[qIdx];
      if (!q) return '(Frage fehlt)';
      if (!entry || entry.length === 0) return '(keine Daten)';

      try {
        // BOOL und NUMERIC: skalares Ergebnis
        if (q.question_type === 'numeric') {
          const raw = this.tfhe.fromBase64(entry[0]);
          const num = this.tfhe.decryptUint32(raw, this.clientKey);
          return `Summe: ${num}`;
        }

        // SINGLE und MULTIPLE: pro Option ein Zähler
        if (q.question_type === 'single' || q.question_type === 'multiple') {
          const opts = q.options ?? [];
          return opts.map((opt, i) => {
            const ct = entry[i];
            if (!ct) return `${opt}: 0`;
            const raw = this.tfhe.fromBase64(ct);
            const count = this.tfhe.decryptUint32(raw, this.clientKey);
            return `${opt}: ${count}`;
          });
        }
        return '(Unbekannter Typ)';
      } catch (e) {
        console.error('Decrypt result failed', e);
        return '(Fehler bei Entschlüsselung)';
      }
    });
  }

  // --- Session beenden ------------------------------------------------------

  finalize(): void {
    this.isFinalizing.set(true);
    this.votingService.finalizeSession(this.sessionId, this.creatorId).subscribe({
      next: () => {
        this.infoMessage.set('Session beendet. Du wirst gleich zurück geleitet...');
        this.stopPolling();
        setTimeout(() => this.router.navigate(['/voting']), 1200);
      },
      error: () => {
        this.errorMessage.set('Session konnte nicht beendet werden.');
        this.isFinalizing.set(false);
      },
    });
  }
}

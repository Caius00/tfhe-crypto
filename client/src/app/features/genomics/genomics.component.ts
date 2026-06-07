import { HttpClient } from '@angular/common/http';
import { Component, OnDestroy, OnInit, computed, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { firstValueFrom } from 'rxjs';
import { KeyPair } from '../../core/crypto/key-pair.model';
import { TfheService } from '../../core/crypto/tfhe.service';

type GenomicsStatus = 'idle' | 'generating' | 'ready' | 'encrypting' | 'processing' | 'result' | 'error';
type ResultKind = 'none' | 'hamming' | 'levenshtein' | 'db-hamming' | 'db-levenshtein' | 'encrypted';
type ResultScope = 'none' | 'single' | 'database';
type SessionRole = 'creator' | 'participant';

interface KeyMaterial {
  keyPair: KeyPair;
  serverKeyB64: string;
}

interface EncryptResponse {
  encrypted_bases: string[];
  original_length: number;
}

interface GenomicsSession {
  id: string;
  public_key: string;
  created_at: string;
}

interface SessionsResponse {
  sessions: GenomicsSession[];
}

interface CreateSessionResponse {
  session: GenomicsSession;
}

interface FifoStatusResponse {
  capacity: number;
  used: number;
  locked: boolean;
  jobs: FifoJob[];
}

interface FifoJob {
  id: number;
  position: number;
  session_id: string;
  job_type: string;
  state: 'queued' | 'running' | string;
}

interface ProcessResponse {
  encrypted_distance_items: string[];
  windows: number;
}

interface CompareDatabaseResponse {
  encrypted_result_items: string[][];
  compared_sequences: number;
  patterns: RiskPattern[];
}

interface PatternsResponse {
  patterns: RiskPattern[];
}

interface CreateRiskPatternResponse {
  pattern: RiskPattern;
}

interface RiskPattern {
  id: number;
  sequence: string;
}

interface WindowResult {
  index: number;
  distance: number;
}

interface DatabaseResult {
  label: string;
  distances: number[];
  bestDistance: number | null;
}

interface KeyOutput {
  label: string;
  value: string;
}

const API_BASE = (() => {
  if (typeof window === 'undefined') return '/genomics';
  const localHosts = new Set(['localhost', '127.0.0.1', '::1']);
  return localHosts.has(window.location.hostname) ? 'http://127.0.0.1:8080' : '/genomics';
})();
const BUSY_STATES: GenomicsStatus[] = ['generating', 'encrypting', 'processing'];
const MAX_HAMMING_SEQUENCE_LENGTH = 255;
const MAX_LEVENSHTEIN_SEQUENCE_LENGTH = 122;
const EMPTY_FIFO_STATUS: FifoStatusResponse = {
  capacity: 4,
  used: 0,
  locked: false,
  jobs: [],
};

@Component({
  selector: 'app-genomics',
  imports: [FormsModule],
  templateUrl: './genomics.component.html',
  styleUrl: './genomics.component.css',
})
export class GenomicsComponent implements OnInit, OnDestroy {
  status = signal<GenomicsStatus>('idle');
  resultKind = signal<ResultKind>('none');
  sequenceInput = signal('ATCGATCGAAAA');
  serverSequenceInput = signal('GGTTAC');
  newPatternInput = signal('');
  selectedPatternId = signal('all');
  riskPatterns = signal<RiskPattern[]>([]);
  encryptedLength = signal(0);
  hammingResults = signal<WindowResult[]>([]);
  levenshteinResult = signal<number | null>(null);
  databaseResults = signal<DatabaseResult[]>([]);
  showResultPanel = signal(false);
  infoMessage = signal('');
  errorMessage = signal('');
  keyOutputItems = signal<KeyOutput[]>([]);
  encryptedResultTitle = signal('');
  encryptedResultItems = signal<KeyOutput[]>([]);
  sequenceOutputTitle = signal('');
  sequenceOutputValue = signal('');
  sessions = signal<GenomicsSession[]>([]);
  activeSession = signal<GenomicsSession | null>(null);
  sessionRole = signal<SessionRole | null>(null);
  fifoStatus = signal<FifoStatusResponse>(EMPTY_FIFO_STATUS);

  private keyReady = signal(false);
  private encryptedSequenceReady = signal(false);
  private resultScope = signal<ResultScope>('none');

  hasSession = computed(() => this.activeSession() !== null);
  hasPrivateKeys = computed(() => this.keyReady());
  hasKeys = computed(() => this.hasSession());
  hasEncryptedSequence = computed(() => this.encryptedSequenceReady());
  hasResult = computed(() => this.resultKind() !== 'none');
  hasSingleResult = computed(() => this.resultScope() === 'single');
  hasHammingMatch = computed(() => this.hammingResults().some((item) => item.distance === 0));
  hasRiskPatterns = computed(() => this.riskPatterns().length > 0);
  hasDatabaseResult = computed(() => this.resultScope() === 'database');
  fifoLocked = computed(() => this.fifoStatus().locked);
  hasNewPatternInput = computed(() => this.normalizeDna(this.newPatternInput()).length > 0);
  databaseMatchCount = computed(
    () => this.databaseResults().filter((entry) => this.entryHasMatch(entry)).length,
  );
  sortedDatabaseResults = computed(() =>
    [...this.databaseResults()].sort((left, right) => {
      const leftMatch = this.entryHasMatch(left);
      const rightMatch = this.entryHasMatch(right);
      if (leftMatch === rightMatch) return 0;
      return leftMatch ? -1 : 1;
    }),
  );
  isBusy = computed(() => BUSY_STATES.includes(this.status()));

  private keyPair: KeyPair | null = null;
  private serverKeyB64 = '';
  private publicKeyB64 = '';
  private encryptedSequenceItems: string[] = [];
  private encryptedSource = '';
  private fifoPollId: ReturnType<typeof setInterval> | null = null;

  constructor(
    private http: HttpClient,
    private tfhe: TfheService,
  ) {}

  ngOnInit(): void {
    void this.loadSessions();
    void this.loadRiskPatterns();
    void this.loadFifoStatus();
    this.fifoPollId = setInterval(() => void this.loadFifoStatus(), 1500);
  }

  ngOnDestroy(): void {
    if (this.fifoPollId) {
      clearInterval(this.fifoPollId);
      this.fifoPollId = null;
    }
  }

  async loadSessions(showMessage = false): Promise<void> {
    const started = this.nowMs();

    try {
      const response = await firstValueFrom(
        this.http.get<SessionsResponse>(`${API_BASE}/sessions`),
      );
      this.sessions.set(response.sessions);

      if (showMessage) {
        this.infoMessage.set(
          `${response.sessions.length} aktive Sessions geladen in ${this.elapsedMs(started)} ms.`,
        );
      }
    } catch (error) {
      this.errorMessage.set(`Sessions konnten nicht geladen werden: ${this.errorText(error)}`);
    }
  }

  async loadFifoStatus(showMessage = false): Promise<void> {
    const started = this.nowMs();

    try {
      const response = await firstValueFrom(
        this.http.get<FifoStatusResponse>(`${API_BASE}/fifo`),
      );
      this.fifoStatus.set(response);

      if (showMessage) {
        this.infoMessage.set(
          `FIFO aktualisiert: ${response.used}/${response.capacity} Auftraege in ${this.elapsedMs(started)} ms.`,
        );
      }
    } catch (error) {
      if (showMessage) {
        this.errorMessage.set(`FIFO konnte nicht geladen werden: ${this.errorText(error)}`);
      }
    }
  }

  async loadRiskPatterns(showMessage = true): Promise<void> {
    const started = this.nowMs();

    try {
      const response = await firstValueFrom(
        this.http.get<PatternsResponse>(`${API_BASE}/patterns`),
      );
      this.riskPatterns.set(response.patterns);

      if (
        this.selectedPatternId() !== 'all' &&
        !response.patterns.some((pattern) => String(pattern.id) === this.selectedPatternId())
      ) {
        this.selectedPatternId.set('all');
      }

      if (showMessage) {
        this.infoMessage.set(
          `${response.patterns.length} Risikomuster aus der Datenbank geladen in ${this.elapsedMs(started)} ms.`,
        );
      }
    } catch (error) {
      this.errorMessage.set(`Risikomuster konnten nicht geladen werden: ${this.errorText(error)}`);
    }
  }

  async addRiskPattern(): Promise<void> {
    const session = this.activeSession();
    if (!session) {
      this.setError('Bitte zuerst eine Session erstellen oder beitreten.');
      return;
    }

    try {
      const cleanPattern = this.normalizeDna(this.newPatternInput());
      this.encodeDna(cleanPattern);
      this.status.set('processing');
      this.clearResults();
      this.clearMessages();
      await this.renderPause();
      const started = this.nowMs();

      const response = await firstValueFrom(
        this.http.post<CreateRiskPatternResponse>(`${API_BASE}/patterns`, {
          sequence: cleanPattern,
          session_id: session.id,
        }),
      );

      this.newPatternInput.set('');
      await this.loadRiskPatterns(false);
      this.selectedPatternId.set(String(response.pattern.id));
      this.status.set('ready');
      this.infoMessage.set(
        `Risikomuster ${response.pattern.sequence} gespeichert in ${this.elapsedMs(started)} ms.`,
      );
      void this.loadFifoStatus();
    } catch (error) {
      this.setError(this.errorText(error));
    }
  }

  async createSession(): Promise<void> {
    this.status.set('generating');
    this.clearMessages();
    await this.renderPause();
    const started = this.nowMs();

    try {
      await this.tfhe.ensureInitialized();
      const generated = this.tfhe.generateKeyPair();

      this.keyPair = generated;
      this.serverKeyB64 = this.tfhe.toBase64(generated.serverKeyBytes);
      this.publicKeyB64 = this.tfhe.toBase64(this.tfhe.generatePublicKey(generated.clientKey));
      const response = await firstValueFrom(
        this.http.post<CreateSessionResponse>(`${API_BASE}/sessions`, {
          public_key: this.publicKeyB64,
          server_key: this.serverKeyB64,
        }),
      );

      this.activeSession.set(response.session);
      this.sessionRole.set('creator');
      this.keyReady.set(true);
      this.encryptedSequenceReady.set(false);
      this.encryptedSequenceItems = [];
      this.encryptedSource = '';
      this.encryptedLength.set(0);
      this.clearResults();
      this.clearKeyOutput();
      this.clearSequenceOutput();

      this.status.set('ready');
      this.infoMessage.set(
        `Session erstellt. Der Client-Key bleibt lokal. Dauer: ${this.elapsedMs(started)} ms.`,
      );
      void this.loadSessions();
    } catch (error) {
      this.setError(`Fehler beim Erstellen der Session: ${this.errorText(error)}`);
    }
  }

  async generateKeys(): Promise<void> {
    await this.createSession();
  }

  joinSession(session: GenomicsSession): void {
    const started = this.nowMs();
    this.keyPair = null;
    this.serverKeyB64 = '';
    this.publicKeyB64 = session.public_key;
    this.activeSession.set(session);
    this.sessionRole.set('participant');
    this.keyReady.set(false);
    this.encryptedSequenceReady.set(false);
    this.encryptedSequenceItems = [];
    this.encryptedSource = '';
    this.encryptedLength.set(0);
    this.status.set('ready');
    this.clearResults();
    this.clearMessages();
    this.clearKeyOutput();
    this.clearSequenceOutput();
    this.infoMessage.set(
      `Session beigetreten. Nur der Public-Key ist verfuegbar. Dauer: ${this.elapsedMs(started)} ms.`,
    );
  }

  leaveSession(): void {
    const started = this.nowMs();
    this.keyPair = null;
    this.serverKeyB64 = '';
    this.publicKeyB64 = '';
    this.activeSession.set(null);
    this.sessionRole.set(null);
    this.keyReady.set(false);
    this.encryptedSequenceReady.set(false);
    this.encryptedSequenceItems = [];
    this.encryptedSource = '';
    this.encryptedLength.set(0);
    this.status.set('idle');
    this.clearResults();
    this.clearMessages();
    this.clearKeyOutput();
    this.clearSequenceOutput();
    this.infoMessage.set(`Session verlassen in ${this.elapsedMs(started)} ms.`);
    void this.loadSessions();
  }

  async encryptSequenceLocal(showMessage = true): Promise<void> {
    const publicKeyB64 = this.currentPublicKeyB64();
    if (!publicKeyB64) {
      this.setError('Bitte zuerst eine Session erstellen oder beitreten.');
      return;
    }

    try {
      const cleanSequence = this.normalizeDna(this.sequenceInput());
      const encoded = this.encodeDna(cleanSequence);
      this.status.set('encrypting');
      this.clearResults();
      this.clearMessages();
      await this.renderPause();
      const started = this.nowMs();

      await this.tfhe.ensureInitialized();
      this.encryptedSequenceItems = this.tfhe.encryptUint8sCompact(publicKeyB64, encoded);
      this.encryptedSource = cleanSequence;
      this.encryptedLength.set(encoded.length);
      this.encryptedSequenceReady.set(true);
      this.clearSequenceOutput();
      this.status.set('ready');

      if (showMessage) {
        this.infoMessage.set(
          `${encoded.length} Basen lokal verschluesselt in ${this.elapsedMs(started)} ms.`,
        );
      }
    } catch (error) {
      this.setError(this.errorText(error));
    }
  }

  async encryptServerSequence(): Promise<void> {
    const session = this.activeSession();
    if (!session) {
      this.setError('Bitte zuerst eine Session erstellen oder beitreten.');
      return;
    }

    try {
      const cleanSequence = this.normalizeDna(this.serverSequenceInput());
      this.encodeDna(cleanSequence);
      this.status.set('encrypting');
      this.clearResults();
      this.clearMessages();
      await this.renderPause();
      const started = this.nowMs();

      const response = await firstValueFrom(
        this.http.post<EncryptResponse>(`${API_BASE}/encrypt`, {
          sequence: cleanSequence,
          session_id: session.id,
        }),
      );

      this.encryptedSequenceItems = response.encrypted_bases;
      this.encryptedSource = cleanSequence;
      this.encryptedLength.set(response.original_length);
      this.encryptedSequenceReady.set(true);
      this.clearSequenceOutput();
      this.sequenceInput.set(cleanSequence);
      this.status.set('ready');
      this.infoMessage.set(
        `${response.original_length} serverseitig mit Public-Key verschluesselt in ${this.elapsedMs(started)} ms.`,
      );
    } catch (error) {
      this.setError(this.errorText(error));
    }
  }

  async computeHamming(): Promise<void> {
    try {
      if (!this.comparisonSequenceWithinLimit(MAX_HAMMING_SEQUENCE_LENGTH, 'Hamming')) return;
      this.status.set('processing');
      this.clearResults();
      this.clearMessages();
      await this.renderPause();
      const started = this.nowMs();
      const body = await this.computeBody(MAX_HAMMING_SEQUENCE_LENGTH, 'Hamming');
      if (!body) return;
      this.status.set('processing');

      const response = await firstValueFrom(
        this.http.post<ProcessResponse>(`${API_BASE}/process`, body),
      );
      if (this.canDecryptResults()) {
        const distances = this.decryptItems(response.encrypted_distance_items);
        this.hammingResults.set(distances.map((distance, index) => ({ index, distance })));
        this.resultKind.set('hamming');
      } else {
        this.setEncryptedResult('Hamming-Ergebnis', [
          {
            label: 'Encrypted Hamming-Distanzen',
            value: response.encrypted_distance_items.join('\n'),
          },
        ]);
      }
      this.resultScope.set('single');
      this.showResultPanel.set(false);
      this.status.set('result');
      this.infoMessage.set(
        `${response.windows} Hamming-Fenster berechnet${this.canDecryptResults() ? ' und lokal entschluesselt' : ''} in ${this.elapsedMs(started)} ms. Result oeffnet die Anzeige.`,
      );
    } catch (error) {
      this.setError(this.errorText(error));
    }
  }

  async computeLevenshtein(): Promise<void> {
    try {
      if (!this.comparisonSequenceWithinLimit(MAX_LEVENSHTEIN_SEQUENCE_LENGTH, 'Levenshtein')) return;
      this.status.set('processing');
      this.clearResults();
      this.clearMessages();
      await this.renderPause();
      const started = this.nowMs();
      const body = await this.computeBody(MAX_LEVENSHTEIN_SEQUENCE_LENGTH, 'Levenshtein');
      if (!body) return;
      this.status.set('processing');

      const response = await firstValueFrom(
        this.http.post<ProcessResponse>(`${API_BASE}/process-levenshtein`, body),
      );
      if (this.canDecryptResults()) {
        const [distance] = this.decryptItems(response.encrypted_distance_items);
        this.levenshteinResult.set(distance ?? null);
        this.resultKind.set('levenshtein');
      } else {
        this.setEncryptedResult('Levenshtein-Ergebnis', [
          {
            label: 'Encrypted Levenshtein-Distanz',
            value: response.encrypted_distance_items.join('\n'),
          },
        ]);
      }
      this.resultScope.set('single');
      this.showResultPanel.set(false);
      this.status.set('result');
      this.infoMessage.set(
        `Levenshtein-Distanz berechnet${this.canDecryptResults() ? ' und lokal entschluesselt' : ''} in ${this.elapsedMs(started)} ms. Result oeffnet die Anzeige.`,
      );
    } catch (error) {
      this.setError(this.errorText(error));
    }
  }

  async compareDatabaseHamming(): Promise<void> {
    try {
      if (!this.comparisonSequenceWithinLimit(MAX_HAMMING_SEQUENCE_LENGTH, 'Hamming')) return;
      this.status.set('processing');
      this.clearResults();
      this.clearMessages();
      await this.renderPause();
      const started = this.nowMs();
      const body = await this.databaseBody(MAX_HAMMING_SEQUENCE_LENGTH, 'Hamming');
      if (!body) return;
      this.status.set('processing');

      const response = await firstValueFrom(
        this.http.post<CompareDatabaseResponse>(`${API_BASE}/compare-db`, body),
      );

      if (this.canDecryptResults()) {
        this.databaseResults.set(
          response.encrypted_result_items.map((items, index) => {
            const distances = this.decryptItems(items);
            const pattern = response.patterns[index];
            return {
              label: pattern ? `${pattern.sequence}` : `DB-Sequenz ${index + 1}`,
              distances,
              bestDistance: distances.length ? Math.min(...distances) : null,
            };
          }),
        );
        this.resultKind.set('db-hamming');
      } else {
        this.setEncryptedResult(
          'Encrypted DB-Hamming-Ergebnisse',
          response.encrypted_result_items.map((items, index) => ({
            label: response.patterns[index]?.sequence ?? `DB-Sequenz ${index + 1}`,
            value: items.join('\n'),
          })),
        );
      }
      this.resultScope.set('database');
      this.showResultPanel.set(false);
      this.status.set('result');
      this.infoMessage.set(
        `${response.compared_sequences} Datenbanksequenzen verglichen${this.canDecryptResults() ? ' und lokal entschluesselt' : ''} in ${this.elapsedMs(started)} ms.`,
      );
    } catch (error) {
      this.setError(this.errorText(error));
    }
  }

  async compareDatabaseLevenshtein(): Promise<void> {
    try {
      if (!this.comparisonSequenceWithinLimit(MAX_LEVENSHTEIN_SEQUENCE_LENGTH, 'Levenshtein')) return;
      this.status.set('processing');
      this.clearResults();
      this.clearMessages();
      await this.renderPause();
      const started = this.nowMs();
      const body = await this.databaseBody(MAX_LEVENSHTEIN_SEQUENCE_LENGTH, 'Levenshtein');
      if (!body) return;
      this.status.set('processing');

      const response = await firstValueFrom(
        this.http.post<CompareDatabaseResponse>(`${API_BASE}/compare-db-levenshtein`, body),
      );

      if (this.canDecryptResults()) {
        this.databaseResults.set(
          response.encrypted_result_items.map((items, index) => {
            const distances = this.decryptItems(items);
            const pattern = response.patterns[index];
            return {
              label: pattern ? `${pattern.sequence}` : `DB-Sequenz ${index + 1}`,
              distances,
              bestDistance: distances[0] ?? null,
            };
          }),
        );
        this.resultKind.set('db-levenshtein');
      } else {
        this.setEncryptedResult(
          'Encrypted DB-Levenshtein-Ergebnisse',
          response.encrypted_result_items.map((items, index) => ({
            label: response.patterns[index]?.sequence ?? `DB-Sequenz ${index + 1}`,
            value: items.join('\n'),
          })),
        );
      }
      this.resultScope.set('database');
      this.showResultPanel.set(false);
      this.status.set('result');
      this.infoMessage.set(
        `${response.compared_sequences} Levenshtein-Vergleiche berechnet${this.canDecryptResults() ? ' und lokal entschluesselt' : ''} in ${this.elapsedMs(started)} ms.`,
      );
    } catch (error) {
      this.setError(this.errorText(error));
    }
  }

  reset(): void {
    const started = this.nowMs();
    this.keyPair = null;
    this.serverKeyB64 = '';
    this.publicKeyB64 = '';
    this.activeSession.set(null);
    this.sessionRole.set(null);
    this.keyReady.set(false);
    this.encryptedSequenceReady.set(false);
    this.encryptedSequenceItems = [];
    this.encryptedSource = '';
    this.encryptedLength.set(0);
    this.status.set('idle');
    this.resultKind.set('none');
    this.clearResults();
    this.clearMessages();
    this.clearKeyOutput();
    this.clearSequenceOutput();
    this.infoMessage.set(`Zurueckgesetzt in ${this.elapsedMs(started)} ms.`);
  }

  updateSequenceInput(value: string): void {
    this.sequenceInput.set(value);
    if (this.normalizeDna(value) !== this.encryptedSource) {
      this.encryptedSequenceReady.set(false);
      this.clearSequenceOutput();
    }
  }

  toggleKeys(): void {
    const started = this.nowMs();
    const publicKeyB64 = this.currentPublicKeyB64();
    if (!publicKeyB64) return;

    this.clearMessages();
    if (this.keyOutputItems().length > 0) {
      this.clearKeyOutput();
      this.infoMessage.set(`Keys geschlossen in ${this.elapsedMs(started)} ms.`);
      return;
    }

    const items: KeyOutput[] = [
      {
        label: 'Public Key',
        value: publicKeyB64,
      },
    ];
    const material = this.keyMaterial();
    if (material) {
      items.unshift({
        label: 'Private Key',
        value: this.tfhe.toBase64(material.keyPair.clientKey.serialize()),
      });
      items.push({
        label: 'Server Key',
        value: material.serverKeyB64,
      });
    }

    this.keyOutputItems.set(items);
    this.infoMessage.set(`Keys angezeigt in ${this.elapsedMs(started)} ms.`);
  }

  showEncryptedSequence(): void {
    const started = this.nowMs();
    if (!this.encryptedSequenceReady() || this.encryptedSequenceItems.length === 0) return;

    this.clearMessages();
    this.sequenceOutputTitle.set('Encrypted DNA-Sequenz');
    this.sequenceOutputValue.set(this.encryptedSequenceItems.join('\n'));
    this.infoMessage.set(`Verschluesselte Sequenz angezeigt in ${this.elapsedMs(started)} ms.`);
  }

  showResult(): void {
    const started = this.nowMs();
    if (!this.hasResult()) return;

    this.clearMessages();
    this.showResultPanel.set(true);
    this.infoMessage.set(`Result angezeigt in ${this.elapsedMs(started)} ms.`);
  }

  entryHasMatch(entry: DatabaseResult): boolean {
    return entry.distances.some((distance) => distance === 0);
  }

  fifoStateLabel(job: FifoJob): string {
    return job.state === 'running' ? 'laeuft' : 'wartet';
  }

  isOwnSessionJob(job: FifoJob): boolean {
    return this.activeSession()?.id === job.session_id;
  }

  private async computeBody(
    maxLength: number,
    operation: string,
  ): Promise<Record<string, number | string | string[] | undefined> | null> {
    const session = this.activeSession();
    if (!session) {
      this.setError('Bitte zuerst eine Session erstellen oder beitreten.');
      return null;
    }
    if (!this.comparisonSequenceWithinLimit(maxLength, operation)) return null;
    const ready = await this.ensureEncryptedSequence();
    if (!ready) return null;
    const selectedPatternId = this.selectedPatternId();

    return {
      encrypted_bases: this.encryptedSequenceItems,
      session_id: session.id,
      pattern_id: selectedPatternId === 'all' ? undefined : Number(selectedPatternId),
    };
  }

  private async databaseBody(
    maxLength: number,
    operation: string,
  ): Promise<Record<string, number | string | string[] | undefined> | null> {
    const session = this.activeSession();
    if (!session) {
      this.setError('Bitte zuerst eine Session erstellen oder beitreten.');
      return null;
    }
    if (!this.comparisonSequenceWithinLimit(maxLength, operation)) return null;
    const ready = await this.ensureEncryptedSequence();
    if (!ready) return null;
    const selectedPatternId = this.selectedPatternId();

    return {
      encrypted_bases: this.encryptedSequenceItems,
      session_id: session.id,
      pattern_id: selectedPatternId === 'all' ? undefined : Number(selectedPatternId),
    };
  }

  private comparisonSequenceWithinLimit(maxLength: number, operation: string): boolean {
    const cleanSequence = this.normalizeDna(this.sequenceInput());
    if (cleanSequence.length > maxLength) {
      this.setError(
        `${operation}: Die Vergleichssequenz darf maximal ${maxLength} Zeichen lang sein.`,
      );
      return false;
    }

    return true;
  }

  private async ensureEncryptedSequence(): Promise<boolean> {
    if (!this.activeSession()) {
      this.setError('Bitte zuerst eine Session erstellen oder beitreten.');
      return false;
    }

    const cleanSequence = this.normalizeDna(this.sequenceInput());
    if (this.encryptedSequenceItems.length === 0 || this.encryptedSource !== cleanSequence) {
      await this.encryptSequenceLocal(false);
      if (this.status() === 'error') return false;
    }

    return true;
  }

  private keyMaterial(): KeyMaterial | null {
    if (!this.keyReady() || !this.keyPair || !this.serverKeyB64) {
      return null;
    }

    return {
      keyPair: this.keyPair,
      serverKeyB64: this.serverKeyB64,
    };
  }

  private currentPublicKeyB64(): string | null {
    if (this.publicKeyB64) return this.publicKeyB64;

    const session = this.activeSession();
    if (session?.public_key) {
      this.publicKeyB64 = session.public_key;
      return this.publicKeyB64;
    }

    if (this.keyPair) {
      return this.ensurePublicKeyB64(this.keyPair);
    }

    return null;
  }

  private ensurePublicKeyB64(keyPair: KeyPair): string {
    if (!this.publicKeyB64) {
      const publicKeyBytes = this.tfhe.generatePublicKey(keyPair.clientKey);
      this.publicKeyB64 = this.tfhe.toBase64(publicKeyBytes);
    }

    return this.publicKeyB64;
  }

  private canDecryptResults(): boolean {
    return this.keyReady() && this.keyPair !== null;
  }

  private setEncryptedResult(title: string, items: KeyOutput[]): void {
    this.encryptedResultTitle.set(title);
    this.encryptedResultItems.set(items);
    this.resultKind.set('encrypted');
  }

  private decryptItems(items: string[]): number[] {
    const material = this.keyMaterial();
    if (!material) {
      throw new Error('Client-Key fehlt.');
    }

    return items.map((item) =>
      this.tfhe.decryptUint8(this.tfhe.fromBase64(item), material.keyPair.clientKey),
    );
  }

  private normalizeDna(sequence: string): string {
    return sequence.toUpperCase().replace(/\s+/g, '');
  }

  private encodeDna(sequence: string): number[] {
    if (!sequence) {
      throw new Error('Bitte eine DNA-Sequenz eingeben.');
    }

    return [...sequence].map((base) => {
      switch (base) {
        case 'A':
          return 0;
        case 'T':
          return 1;
        case 'C':
          return 2;
        case 'G':
          return 3;
        default:
          throw new Error(`Ungueltige Base "${base}". Erlaubt sind A, T, C und G.`);
      }
    });
  }

  private clearResults(): void {
    this.hammingResults.set([]);
    this.levenshteinResult.set(null);
    this.databaseResults.set([]);
    this.encryptedResultTitle.set('');
    this.encryptedResultItems.set([]);
    this.resultKind.set('none');
    this.resultScope.set('none');
    this.showResultPanel.set(false);
  }

  private clearMessages(): void {
    this.infoMessage.set('');
    this.errorMessage.set('');
  }

  private clearKeyOutput(): void {
    this.keyOutputItems.set([]);
  }

  private clearSequenceOutput(): void {
    this.sequenceOutputTitle.set('');
    this.sequenceOutputValue.set('');
  }

  private setError(message: string): void {
    this.errorMessage.set(message);
    this.infoMessage.set('');
    this.status.set('error');
  }

  private errorText(error: unknown): string {
    if (error instanceof Error) return error.message;
    if (typeof error === 'object' && error !== null) {
      const maybeHttp = error as { error?: unknown; message?: unknown };
      if (typeof maybeHttp.error === 'string' && maybeHttp.error.trim()) return maybeHttp.error;
      if (typeof maybeHttp.message === 'string' && maybeHttp.message.trim()) return maybeHttp.message;
    }
    return 'Unbekannter Fehler';
  }

  private async renderPause(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, 50));
  }

  private nowMs(): number {
    return typeof performance === 'undefined' ? Date.now() : performance.now();
  }

  private elapsedMs(started: number): number {
    return Math.max(0, Math.round(this.nowMs() - started));
  }
}

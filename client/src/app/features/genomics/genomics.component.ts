import { HttpClient } from '@angular/common/http';
import { Component, OnInit, computed, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { firstValueFrom } from 'rxjs';
import { KeyPair } from '../../core/crypto/key-pair.model';
import { TfheService } from '../../core/crypto/tfhe.service';

type GenomicsStatus = 'idle' | 'generating' | 'ready' | 'encrypting' | 'processing' | 'result' | 'error';
type ResultKind = 'none' | 'hamming' | 'levenshtein' | 'db-hamming' | 'db-levenshtein';

interface KeyMaterial {
  keyPair: KeyPair;
  serverKeyB64: string;
}

interface EncryptResponse {
  encrypted_bases: string[];
  original_length: number;
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

@Component({
  selector: 'app-genomics',
  imports: [FormsModule],
  templateUrl: './genomics.component.html',
  styleUrl: './genomics.component.css',
})
export class GenomicsComponent implements OnInit {
  status = signal<GenomicsStatus>('idle');
  resultKind = signal<ResultKind>('none');
  sequenceInput = signal('ATCGATCGAAAA');
  serverSequenceInput = signal('GGTTAC');
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
  sequenceOutputTitle = signal('');
  sequenceOutputValue = signal('');

  private keyReady = signal(false);
  private encryptedSequenceReady = signal(false);

  hasKeys = computed(() => this.keyReady());
  hasEncryptedSequence = computed(() => this.encryptedSequenceReady());
  hasResult = computed(() => this.resultKind() !== 'none');
  hasSingleResult = computed(() => this.resultKind() === 'hamming' || this.resultKind() === 'levenshtein');
  hasHammingMatch = computed(() => this.hammingResults().some((item) => item.distance === 0));
  hasRiskPatterns = computed(() => this.riskPatterns().length > 0);
  hasDatabaseResult = computed(() => this.resultKind() === 'db-hamming' || this.resultKind() === 'db-levenshtein');
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

  constructor(
    private http: HttpClient,
    private tfhe: TfheService,
  ) {}

  ngOnInit(): void {
    void this.loadRiskPatterns();
  }

  async loadRiskPatterns(): Promise<void> {
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

      this.infoMessage.set(
        `${response.patterns.length} Risikomuster aus der Datenbank geladen in ${this.elapsedMs(started)} ms.`,
      );
    } catch (error) {
      this.errorMessage.set(`Risikomuster konnten nicht geladen werden: ${this.errorText(error)}`);
    }
  }

  async generateKeys(): Promise<void> {
    this.status.set('generating');
    this.clearMessages();
    await this.renderPause();
    const started = this.nowMs();

    try {
      await this.tfhe.ensureInitialized();
      const generated = this.tfhe.generateKeyPair();

      this.keyPair = generated;
      this.serverKeyB64 = this.tfhe.toBase64(generated.serverKeyBytes);
      this.publicKeyB64 = '';
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
        `Keyset bereit. Der Client-Key bleibt lokal. Dauer: ${this.elapsedMs(started)} ms.`,
      );
    } catch (error) {
      this.setError(`Fehler bei der Schluesselgenerierung: ${this.errorText(error)}`);
    }
  }

  async encryptSequenceLocal(showMessage = true): Promise<void> {
    const material = this.keyMaterial();
    if (!material) {
      this.setError('Bitte zuerst ein Client-Keyset erzeugen.');
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

      this.encryptedSequenceItems = encoded.map((value) =>
        this.tfhe.toBase64(this.tfhe.encryptUint8(value, material.keyPair.clientKey)),
      );
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
    const material = this.keyMaterial();
    if (!material) {
      this.setError('Bitte zuerst ein Client-Keyset erzeugen.');
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
      const publicKeyB64 = this.ensurePublicKeyB64(material.keyPair);

      const response = await firstValueFrom(
        this.http.post<EncryptResponse>(`${API_BASE}/encrypt`, {
          sequence: cleanSequence,
          public_key: publicKeyB64,
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
      this.status.set('processing');
      this.clearResults();
      this.clearMessages();
      await this.renderPause();
      const started = this.nowMs();
      const body = await this.computeBody();
      if (!body) return;
      this.status.set('processing');

      const response = await firstValueFrom(
        this.http.post<ProcessResponse>(`${API_BASE}/process`, body),
      );
      const distances = this.decryptItems(response.encrypted_distance_items);
      this.hammingResults.set(distances.map((distance, index) => ({ index, distance })));
      this.resultKind.set('hamming');
      this.showResultPanel.set(false);
      this.status.set('result');
      this.infoMessage.set(
        `${response.windows} Hamming-Fenster berechnet und lokal entschluesselt in ${this.elapsedMs(started)} ms. Result oeffnet die Anzeige.`,
      );
    } catch (error) {
      this.setError(this.errorText(error));
    }
  }

  async computeLevenshtein(): Promise<void> {
    try {
      this.status.set('processing');
      this.clearResults();
      this.clearMessages();
      await this.renderPause();
      const started = this.nowMs();
      const body = await this.computeBody();
      if (!body) return;
      this.status.set('processing');

      const response = await firstValueFrom(
        this.http.post<ProcessResponse>(`${API_BASE}/process-levenshtein`, body),
      );
      const [distance] = this.decryptItems(response.encrypted_distance_items);
      this.levenshteinResult.set(distance ?? null);
      this.resultKind.set('levenshtein');
      this.showResultPanel.set(false);
      this.status.set('result');
      this.infoMessage.set(
        `Levenshtein-Distanz berechnet und lokal entschluesselt in ${this.elapsedMs(started)} ms. Result oeffnet die Anzeige.`,
      );
    } catch (error) {
      this.setError(this.errorText(error));
    }
  }

  async compareDatabaseHamming(): Promise<void> {
    try {
      this.status.set('processing');
      this.clearResults();
      this.clearMessages();
      await this.renderPause();
      const started = this.nowMs();
      const body = await this.databaseBody();
      if (!body) return;
      this.status.set('processing');

      const response = await firstValueFrom(
        this.http.post<CompareDatabaseResponse>(`${API_BASE}/compare-db`, body),
      );

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
      this.showResultPanel.set(false);
      this.status.set('result');
      this.infoMessage.set(
        `${response.compared_sequences} Datenbanksequenzen verglichen und lokal entschluesselt in ${this.elapsedMs(started)} ms.`,
      );
    } catch (error) {
      this.setError(this.errorText(error));
    }
  }

  async compareDatabaseLevenshtein(): Promise<void> {
    try {
      this.status.set('processing');
      this.clearResults();
      this.clearMessages();
      await this.renderPause();
      const started = this.nowMs();
      const body = await this.databaseBody();
      if (!body) return;
      this.status.set('processing');

      const response = await firstValueFrom(
        this.http.post<CompareDatabaseResponse>(`${API_BASE}/compare-db-levenshtein`, body),
      );

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
      this.showResultPanel.set(false);
      this.status.set('result');
      this.infoMessage.set(
        `${response.compared_sequences} Levenshtein-Vergleiche berechnet und lokal entschluesselt in ${this.elapsedMs(started)} ms.`,
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
    const material = this.keyMaterial();
    if (!material) return;

    this.clearMessages();
    if (this.keyOutputItems().length > 0) {
      this.clearKeyOutput();
      this.infoMessage.set(`Keys geschlossen in ${this.elapsedMs(started)} ms.`);
      return;
    }

    this.keyOutputItems.set([
      {
        label: 'Private Key',
        value: this.tfhe.toBase64(material.keyPair.clientKey.serialize()),
      },
      {
        label: 'Public Key',
        value: this.ensurePublicKeyB64(material.keyPair),
      },
      {
        label: 'Server Key',
        value: material.serverKeyB64,
      },
    ]);
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

  private async computeBody(): Promise<Record<string, string | string[] | undefined> | null> {
    const material = await this.ensureEncryptedSequence();
    if (!material) return null;

    const publicKeyB64 = this.ensurePublicKeyB64(material.keyPair);

    return {
      encrypted_bases: this.encryptedSequenceItems,
      server_key: material.serverKeyB64,
      public_key: publicKeyB64,
    };
  }

  private async databaseBody(): Promise<Record<string, number | string | string[] | undefined> | null> {
    const material = await this.ensureEncryptedSequence();
    if (!material) return null;
    const publicKeyB64 = this.ensurePublicKeyB64(material.keyPair);
    const selectedPatternId = this.selectedPatternId();

    return {
      encrypted_bases: this.encryptedSequenceItems,
      server_key: material.serverKeyB64,
      public_key: publicKeyB64,
      pattern_id: selectedPatternId === 'all' ? undefined : Number(selectedPatternId),
    };
  }

  private async ensureEncryptedSequence(): Promise<KeyMaterial | null> {
    const material = this.keyMaterial();
    if (!material) {
      this.setError('Bitte zuerst ein Client-Keyset erzeugen.');
      return null;
    }

    const cleanSequence = this.normalizeDna(this.sequenceInput());
    if (this.encryptedSequenceItems.length === 0 || this.encryptedSource !== cleanSequence) {
      await this.encryptSequenceLocal(false);
      if (this.status() === 'error') return null;
    }

    return material;
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

  private ensurePublicKeyB64(keyPair: KeyPair): string {
    if (!this.publicKeyB64) {
      const publicKeyBytes = this.tfhe.generatePublicKey(keyPair.clientKey);
      this.publicKeyB64 = this.tfhe.toBase64(publicKeyBytes);
    }

    return this.publicKeyB64;
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
    this.resultKind.set('none');
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

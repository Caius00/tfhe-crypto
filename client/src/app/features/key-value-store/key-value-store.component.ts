import { Component, OnInit, signal } from '@angular/core';
import { DatePipe } from '@angular/common';
import { TfheService } from '../../core/crypto/tfhe.service';
import { KeyValueStoreApiService } from '../../core/api/key-value-store-api.service';
import { KeyPair } from '../../core/crypto/key-pair.model';
import { ButtonComponent } from '../../shared/components/button/button.component';
import { InputComponent } from '../../shared/components/input/input.component';
import { SpinnerComponent } from '../../shared/components/spinner/spinner.component';

/**
 * UI-Schritte. Eine einzige Komponente hält den ganzen Lifecycle der Session;
 * die verschiedenen Karten (Put/Get/Exists/Clear) werden über `step` ein- und
 * ausgeblendet.
 */
type Step = 'init' | 'generating' | 'ready' | 'working' | 'error';

/** Lokaler Spiegel eines Eintrags. Der Client kennt seine eigenen Klartexte;
 *  der Server sieht ausschließlich die verschlüsselten Bytes. */
interface LocalEntry {
  key: string;
  value: string;
  ttlSeconds: number;
  /** ISO-Zeitstempel beim Put — für Anzeige "wann gespeichert" */
  storedAt: string;
}

/** Cache-Schlüssel im sessionStorage — der KV-Store hat einen eigenen Slot,
 *  damit er sich nicht mit anderen Features kollidiert. Wir speichern nur den
 *  Client-Key (zum Entschlüsseln) und die Session-ID; der Server-Key liegt
 *  serverseitig und wird nach dem initialen Upload nicht mehr gebraucht — das
 *  hält die Persistenz unter dem sessionStorage-Quota. */
const SESSION_STORAGE_KEYS = {
  clientKey: 'kv_client_key',
  sessionId: 'kv_session_id',
  entries: 'kv_entries',
} as const;

@Component({
  selector: 'app-key-value-store',
  imports: [ButtonComponent, InputComponent, SpinnerComponent, DatePipe],
  templateUrl: './key-value-store.component.html',
  styleUrl: './key-value-store.component.css',
})
export class KeyValueStoreComponent implements OnInit {
  // -------- View-State (Signals) --------
  step = signal<Step>('init');
  errorMessage = signal('');
  /** Statustext der gerade laufenden Operation — für die Loading-Karte. */
  busyMessage = signal('');

  // Formularfelder
  putKey = signal('');
  putValue = signal('');
  putTtl = signal('300');
  getKey = signal('');
  existsKey = signal('');

  // Ergebnisse der letzten Operation
  lastGetResult = signal<{ key: string; value: string } | null>(null);
  lastExistsResult = signal<{ key: string; exists: boolean } | null>(null);

  /** Lokal gespiegelte Einträge — Klartext, da der Client sie selbst gesendet hat. */
  entries = signal<LocalEntry[]>([]);

  /** Die Session-ID, unter der der Server unseren ServerKey kennt. */
  sessionId = signal<string | null>(null);

  /**
   * Schlüsselpaar bleibt absichtlich außerhalb des Signal-Systems: es ist nicht
   * UI-reaktiv (wir rendern keinen Inhalt davon), nur ein langlebiges Objekt.
   */
  private keyPair: KeyPair | null = null;

  constructor(
    private tfhe: TfheService,
    private api: KeyValueStoreApiService,
  ) {}

  ngOnInit(): void {
    // Beim Mount versuchen, eine bestehende Session aus sessionStorage zu
    // restaurieren. Wenn alles vorhanden ist, springen wir direkt in den
    // `ready`-Schritt und ersparen dem Nutzer die Keygen-Wartezeit.
    void this.tryRehydrate();
  }

  private async tryRehydrate(): Promise<void> {
    const storedSession = sessionStorage.getItem(SESSION_STORAGE_KEYS.sessionId);
    const storedClientKey = sessionStorage.getItem(SESSION_STORAGE_KEYS.clientKey);
    if (!storedSession || !storedClientKey) return;

    try {
      await this.tfhe.ensureInitialized();
      const clientKey = this.tfhe.deserializeClientKey(this.tfhe.fromBase64(storedClientKey));
      // serverKeyBytes brauchen wir nach dem initialen Upload nicht mehr — der
      // Server hält den dekomprimierten ServerKey unter der session_id. Ein
      // leerer Buffer reicht als Platzhalter im KeyPair.
      this.keyPair = { clientKey, serverKeyBytes: new Uint8Array() };
      this.sessionId.set(storedSession);
      this.loadEntriesFromStorage();
      this.step.set('ready');
    } catch (e) {
      // Korrupter Eintrag oder TFHE-Init-Fehler — Tabula rasa, der Nutzer
      // kann jederzeit eine neue Session starten.
      this.resetSessionStorage();
    }
  }

  // ---------------------------------------------------------------------------
  // Session-Lifecycle
  // ---------------------------------------------------------------------------

  /**
   * Generiert ein neues KeyPair (30–90 s) und öffnet darauf eine Session am
   * Server. ServerKey wird base64-kodiert und einmalig hochgeladen.
   */
  async startSession(): Promise<void> {
    this.step.set('generating');
    this.busyMessage.set('Schlüssel werden generiert …');
    // Kurze Pause, damit Angular die Loading-Karte rendert bevor der Main-Thread
    // ~Minuten von WASM-FHE blockiert wird.
    await new Promise((resolve) => setTimeout(resolve, 50));

    try {
      await this.tfhe.ensureInitialized();
      this.keyPair = this.tfhe.generateKeyPair();

      this.busyMessage.set('Session wird am Server angelegt …');
      const serverKeyB64 = this.tfhe.toBase64(this.keyPair.serverKeyBytes);

      this.api.createSession(serverKeyB64).subscribe({
        next: (sessionId) => {
          this.sessionId.set(sessionId);
          // Nur Client-Key + Session-ID persistieren — der Server-Key liegt ab
          // jetzt am Server und braucht im Browser nicht mehr aufbewahrt zu
          // werden. So bleibt die Persistenz unter dem sessionStorage-Quota.
          this.persistSession(sessionId);
          this.entries.set([]);
          this.saveEntriesToStorage();
          this.step.set('ready');
        },
        error: (err) => this.fail(`Session-Anlegen fehlgeschlagen: ${this.errMsg(err)}`),
      });
    } catch (e) {
      console.error('Keygen failed:', e);
      this.fail(`Fehler beim Generieren der Schlüssel: ${this.errMsg(e)}`);
    }
  }

  /**
   * Löscht ServerKey, ClientKey und lokale Einträge — sowohl im Browser-Speicher
   * als auch optional am Server. Danach landet der Nutzer wieder im
   * Initial-Bildschirm.
   */
  reset(): void {
    this.resetSessionStorage();
    this.keyPair = null;
    this.sessionId.set(null);
    this.entries.set([]);
    this.lastGetResult.set(null);
    this.lastExistsResult.set(null);
    this.putKey.set('');
    this.putValue.set('');
    this.putTtl.set('300');
    this.getKey.set('');
    this.existsKey.set('');
    this.errorMessage.set('');
    this.step.set('init');
  }

  private resetSessionStorage(): void {
    sessionStorage.removeItem(SESSION_STORAGE_KEYS.clientKey);
    sessionStorage.removeItem(SESSION_STORAGE_KEYS.sessionId);
    sessionStorage.removeItem(SESSION_STORAGE_KEYS.entries);
  }

  /** Persistiert Client-Key und Session-ID; Fehler (Quota) werden geschluckt. */
  private persistSession(sessionId: string): void {
    if (!this.keyPair) return;
    try {
      const clientKeyB64 = this.tfhe.toBase64(
        this.tfhe.serializeClientKey(this.keyPair.clientKey),
      );
      sessionStorage.setItem(SESSION_STORAGE_KEYS.clientKey, clientKeyB64);
      sessionStorage.setItem(SESSION_STORAGE_KEYS.sessionId, sessionId);
    } catch (err) {
      // Funktional egal: Reload-Persistenz schlägt fehl, aktive Session läuft
      // trotzdem weiter — der Server-Key bleibt drüben unter der session_id.
      console.warn('persistSession failed (continuing):', err);
    }
  }

  // ---------------------------------------------------------------------------
  // Operationen — Put / Get / Exists / Clear
  // ---------------------------------------------------------------------------

  /** Verschlüsselt key+value zeichenweise und schickt sie ans Backend. */
  async runPut(): Promise<void> {
    if (!this.ready()) return;
    const key = this.putKey().trim();
    const value = this.putValue();
    const ttlInput = this.putTtl().trim();

    if (key.length === 0) {
      this.fail('Schlüssel darf nicht leer sein.');
      return;
    }

    let ttlSeconds: number | undefined = undefined;
    if (ttlInput.length > 0) {
      const parsed = Number.parseInt(ttlInput, 10);
      if (Number.isNaN(parsed) || parsed <= 0) {
        this.fail('TTL muss eine positive ganze Zahl in Sekunden sein.');
        return;
      }
      ttlSeconds = parsed;
    }

    this.step.set('working');
    this.busyMessage.set(`„${key}" wird verschlüsselt und gespeichert …`);
    await new Promise((resolve) => setTimeout(resolve, 30));

    try {
      const keyChunks = this.tfhe.encryptStringWithClientKey(key, this.keyPair!.clientKey);
      const valueChunks = this.tfhe.encryptStringWithClientKey(value, this.keyPair!.clientKey);

      this.api.put(this.sessionId()!, keyChunks, valueChunks, ttlSeconds).subscribe({
        next: () => {
          const entry: LocalEntry = {
            key,
            value,
            ttlSeconds: ttlSeconds ?? -1,
            storedAt: new Date().toISOString(),
          };
          this.entries.update((list) => [entry, ...list]);
          this.saveEntriesToStorage();
          this.putKey.set('');
          this.putValue.set('');
          this.step.set('ready');
        },
        error: (err) => this.fail(`Put fehlgeschlagen: ${this.errMsg(err)}`),
      });
    } catch (e) {
      this.fail('Fehler bei der Verschlüsselung des Eintrags.');
    }
  }

  /** Holt den verschlüsselten Wert und entschlüsselt lokal. */
  async runGet(): Promise<void> {
    if (!this.ready()) return;
    const key = this.getKey().trim();
    if (key.length === 0) {
      this.fail('Bitte einen Schlüssel zum Lesen angeben.');
      return;
    }

    this.step.set('working');
    this.busyMessage.set(`„${key}" wird homomorph gesucht …`);
    await new Promise((resolve) => setTimeout(resolve, 30));

    const keyChunks = this.tfhe.encryptStringWithClientKey(key, this.keyPair!.clientKey);
    this.api.get(this.sessionId()!, keyChunks).subscribe({
      next: (valueChunks) => {
        const decrypted = this.tfhe.decryptStringFromChunks(valueChunks, this.keyPair!.clientKey);
        this.lastGetResult.set({ key, value: decrypted });
        this.step.set('ready');
      },
      error: (err) => this.fail(`Get fehlgeschlagen: ${this.errMsg(err)}`),
    });
  }

  /** Liefert verschlüsseltes Bool zurück, das wir lokal entschlüsseln. */
  async runExists(): Promise<void> {
    if (!this.ready()) return;
    const key = this.existsKey().trim();
    if (key.length === 0) {
      this.fail('Bitte einen Schlüssel angeben.');
      return;
    }

    this.step.set('working');
    this.busyMessage.set(`Existenz von „${key}" wird homomorph geprüft …`);
    await new Promise((resolve) => setTimeout(resolve, 30));

    const keyChunks = this.tfhe.encryptStringWithClientKey(key, this.keyPair!.clientKey);
    this.api.exists(this.sessionId()!, keyChunks).subscribe({
      next: (existsB64) => {
        const bytes = this.tfhe.fromBase64(existsB64);
        const exists = this.tfhe.decryptBool(bytes, this.keyPair!.clientKey);
        this.lastExistsResult.set({ key, exists });
        this.step.set('ready');
      },
      error: (err) => this.fail(`Exists fehlgeschlagen: ${this.errMsg(err)}`),
    });
  }

  /** Leert ausschließlich die eigene Session am Server. */
  runClear(): void {
    if (!this.ready()) return;
    this.step.set('working');
    this.busyMessage.set('Eigene Session am Server wird geleert …');

    this.api.clear(this.sessionId()!).subscribe({
      next: () => {
        this.entries.set([]);
        this.saveEntriesToStorage();
        this.lastGetResult.set(null);
        this.lastExistsResult.set(null);
        this.step.set('ready');
      },
      error: (err) => this.fail(`Clear fehlgeschlagen: ${this.errMsg(err)}`),
    });
  }

  // ---------------------------------------------------------------------------
  // Helpers
  // ---------------------------------------------------------------------------

  /** Plausibilitäts-Check vor jeder Operation. */
  private ready(): boolean {
    if (!this.keyPair || !this.sessionId()) {
      this.fail('Keine aktive Session — bitte zuerst eine Session starten.');
      return false;
    }
    return true;
  }

  /** Fehler in einen einheitlichen "error"-Schritt überführen. */
  private fail(message: string): void {
    this.errorMessage.set(message);
    this.step.set('error');
  }

  /** Extrahiert eine lesbare Fehlermeldung aus HttpErrorResponse oder Error. */
  private errMsg(err: unknown): string {
    if (typeof err === 'object' && err !== null) {
      const httpErr = err as { error?: { message?: string }; message?: string };
      return httpErr.error?.message ?? httpErr.message ?? 'Unbekannter Fehler';
    }
    return String(err);
  }

  private saveEntriesToStorage(): void {
    try {
      sessionStorage.setItem(SESSION_STORAGE_KEYS.entries, JSON.stringify(this.entries()));
    } catch {
      // sessionStorage kann voll sein — Funktional egal, wir verlieren nur den
      // Reload-Spiegel der lokalen Liste.
    }
  }

  private loadEntriesFromStorage(): void {
    const raw = sessionStorage.getItem(SESSION_STORAGE_KEYS.entries);
    if (!raw) return;
    try {
      const parsed = JSON.parse(raw) as LocalEntry[];
      if (Array.isArray(parsed)) this.entries.set(parsed);
    } catch {
      // Korrupter Eintrag — ignorieren, der Server bleibt die Quelle der Wahrheit.
    }
  }

  /** Vom Error-Schritt zurück in ready oder init. */
  dismissError(): void {
    this.errorMessage.set('');
    this.step.set(this.sessionId() ? 'ready' : 'init');
  }
}

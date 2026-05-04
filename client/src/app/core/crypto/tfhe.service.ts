import { Injectable } from '@angular/core';
import init, {
  FheBool,
  FheInt8,
  FheInt16,
  FheInt32,
  FheInt64,
  FheUint8,
  FheUint16,
  FheUint32,
  FheUint64,
  TfheClientKey,
  TfheCompressedServerKey,
  TfheConfigBuilder,
  TfheCompressedPublicKey, // anstelle des PublicKeys
} from 'tfhe';
import { KeyPair } from './key-pair.model';

const SESSION_KEY_CLIENT = 'tfhe_client_key';
const SESSION_KEY_SERVER = 'tfhe_server_key';

@Injectable({ providedIn: 'root' })
export class TfheService {
  private wasmInitialized = false;

  // ---------------------------------------------------------------------------
  // Initialisierung
  // ---------------------------------------------------------------------------

  /**
   * Lädt die WASM-Datei einmalig. Muss vor allen anderen Operationen
   * aufgerufen werden. Mehrfache Aufrufe sind sicher (wird nur einmal geladen).
   */
  async ensureInitialized(): Promise<void> {
    if (this.wasmInitialized) return;
    await init('/tfhe_bg.wasm');
    this.wasmInitialized = true;
  }

  // ---------------------------------------------------------------------------
  // Schlüsselgenerierung
  // ---------------------------------------------------------------------------

  /**
   * Generiert ein neues Schlüsselpaar (Client-Key + Compressed-Server-Key).
   *
   * - Client-Key: bleibt immer lokal im Browser, wird zum Entschlüsseln genutzt
   * - Server-Key: wird an den Backend-Service geschickt, der damit blind rechnet
   *
   * Achtung: Blockiert den Haupt-Thread für 30–90 Sekunden.
   * Wird für alle homomorphen Berechnungen benötigt.
   */
  generateKeyPair(): KeyPair {
    const config = TfheConfigBuilder.default().build();

    // 1) Client-Key (privat, bleibt im Browser)
    const clientKey = TfheClientKey.generate(config);

    // 2) Server-Key (für Backend-Berechnungen)
    const compressedServerKey = TfheCompressedServerKey.new(clientKey);
    const serverKeyBytes = compressedServerKey.serialize();
    compressedServerKey.free();

    // 3) Public-Key (für andere Clients)
    const compressedPublicKey = TfheCompressedPublicKey.new(clientKey);
    const publicKeyBytes = compressedPublicKey.serialize();
    compressedPublicKey.free();

    return { clientKey, serverKeyBytes, publicKeyBytes };
  }

  private deserializePublicKeyFromB64(publicKeyB64: string): TfheCompressedPublicKey {
    const bytes = this.fromBase64(publicKeyB64);
    return TfheCompressedPublicKey.deserialize(bytes);
  }

  encryptBoolWithPublic(publicKeyB64: string, value: boolean): Uint8Array {
    const bytes = this.fromBase64(publicKeyB64);
    const pk = TfheCompressedPublicKey.deserialize(bytes);
    const enc = FheBool.encrypt_with_compressed_public_key(value, pk);
    const result = enc.serialize();
    enc.free();
    pk.free();
    return result;
  }

  encryptUint8WithPublic(publicKeyB64: string, value: number): Uint8Array {
    const bytes = this.fromBase64(publicKeyB64);
    const pk = TfheCompressedPublicKey.deserialize(bytes);
    const enc = FheUint8.encrypt_with_compressed_public_key(value, pk);
    const result = enc.serialize();
    enc.free();
    pk.free();
    return result;
  }

  // String -> Base64-Chunks (string[])
  encryptStringWithPublic(publicKeyB64: string, text: string): string[] {
    const bytes = this.fromBase64(publicKeyB64);
    const pk = TfheCompressedPublicKey.deserialize(bytes);
    const chunks: string[] = [];

    for (const ch of text) {
      const code = ch.charCodeAt(0);
      const enc = FheUint8.encrypt_with_compressed_public_key(code, pk);
      const encBytes = enc.serialize();
      chunks.push(this.toBase64(encBytes));
      enc.free();
    }

    pk.free();
    return chunks;
  }

  /**
   * Generiert ein reproduzierbares Schlüsselpaar anhand eines Seed-Werts.
   * Mit demselben Seed entsteht immer dasselbe Schlüsselpaar.
   *
   * Nützlich für: Tests, Demo-Szenarien, Debugging
   * Nicht für Produktion empfohlen (vorhersehbare Schlüssel).
   */
  generateKeyPairFromSeed(seed: bigint): KeyPair {
    const config = TfheConfigBuilder.default().build();
    const clientKey = TfheClientKey.generate_with_seed(config, seed);
    const compressedServerKey = TfheCompressedServerKey.new(clientKey);
    const serverKeyBytes = compressedServerKey.serialize();
    compressedServerKey.free();
    const publicKey = TfheCompressedPublicKey.new(clientKey);
    const publicKeyBytes = publicKey.serialize();
    publicKey.free();
    return { clientKey, serverKeyBytes, publicKeyBytes };
  }

  // ---------------------------------------------------------------------------
  // Schlüssel-Persistenz (sessionStorage)
  // Schlüssel bleiben bis zum Schließen des Tabs erhalten.
  // Beim nächsten Seitenaufruf muss nicht neu generiert werden.
  // ---------------------------------------------------------------------------

  /**
   * Speichert das Schlüsselpaar im sessionStorage des Browsers.
   * Bleibt bis zum Schließen des Tabs erhalten.
   */
  saveKeyPairToSession(keyPair: KeyPair): void {
    const clientKeyBytes = keyPair.clientKey.serialize();
    sessionStorage.setItem(SESSION_KEY_CLIENT, this.toBase64(clientKeyBytes));
    sessionStorage.setItem(SESSION_KEY_SERVER, this.toBase64(keyPair.serverKeyBytes));
    sessionStorage.setItem('tfhe_public_key', this.toBase64(keyPair.publicKeyBytes));
  }

  /**
   * Stellt ein gespeichertes Schlüsselpaar aus dem sessionStorage wieder her.
   * Gibt null zurück wenn kein gespeichertes Paar vorhanden ist.
   */
  loadKeyPairFromSession(): KeyPair | null {
    const clientB64 = sessionStorage.getItem(SESSION_KEY_CLIENT);
    const serverB64 = sessionStorage.getItem(SESSION_KEY_SERVER);
    const publicB64 = sessionStorage.getItem('tfhe_public_key');

    if (!clientB64 || !serverB64 || !publicB64) return null;

    const clientKey = TfheClientKey.deserialize(this.fromBase64(clientB64));
    const serverKeyBytes = this.fromBase64(serverB64);
    const publicKeyBytes = this.fromBase64(publicB64);

    return { clientKey, serverKeyBytes, publicKeyBytes };
  }

  /**
   * Löscht das gespeicherte Schlüsselpaar aus dem sessionStorage.
   */
  clearKeyPairFromSession(): void {
    sessionStorage.removeItem(SESSION_KEY_CLIENT);
    sessionStorage.removeItem(SESSION_KEY_SERVER);
    sessionStorage.removeItem('tfhe_public_key');
  }

  // ---------------------------------------------------------------------------
  // Verschlüsselung – Vorzeichenlos (Unsigned)
  // Für Werte die nie negativ sind (Alter, Zähler, Scores, Pixelwerte, ...)
  // ---------------------------------------------------------------------------

  /**
   * Verschlüsselt einen vorzeichenlosen 8-Bit-Wert (0–255).
   *
   * Geeignet für: Alter, Pixelwerte, Genomik-Daten, kleine Zähler
   * Beispiel: Alter-Verification, Image-Processing, Genomics
   */
  encryptUint8(value: number, clientKey: TfheClientKey): Uint8Array {
    const enc = FheUint8.encrypt_with_client_key(value, clientKey);
    const bytes = enc.serialize();
    enc.free();
    return bytes;
  }

  /**
   * Verschlüsselt einen vorzeichenlosen 16-Bit-Wert (0–65.535).
   *
   * Geeignet für: Stimmenanzahl, mittelgroße Scores, Ports
   * Beispiel: Voting/Polling, Leaderboard
   */
  encryptUint16(value: number, clientKey: TfheClientKey): Uint8Array {
    const enc = FheUint16.encrypt_with_client_key(value, clientKey);
    const bytes = enc.serialize();
    enc.free();
    return bytes;
  }

  /**
   * Verschlüsselt einen vorzeichenlosen 32-Bit-Wert (0–4.294.967.295).
   *
   * Geeignet für: Auktionsgebote, große Zähler, Hashwerte
   * Beispiel: Sealed-Bid Auction, Statistics Service
   */
  encryptUint32(value: number, clientKey: TfheClientKey): Uint8Array {
    const enc = FheUint32.encrypt_with_client_key(value, clientKey);
    const bytes = enc.serialize();
    enc.free();
    return bytes;
  }

  /**
   * Verschlüsselt einen vorzeichenlosen 64-Bit-Wert als BigInt.
   *
   * Geeignet für: sehr große Zahlen, Zeitstempel, Programmzähler
   * Beispiel: Encrypted Program Execution, Key-Value Store
   * Hinweis: JavaScript number reicht nicht aus → BigInt verwenden
   */
  encryptUint64(value: bigint, clientKey: TfheClientKey): Uint8Array {
    const enc = FheUint64.encrypt_with_client_key(value, clientKey);
    const bytes = enc.serialize();
    enc.free();
    return bytes;
  }

  // ---------------------------------------------------------------------------
  // Verschlüsselung – Vorzeichenbehaftet (Signed)
  // Für Werte die auch negativ sein können (Temperaturen, Differenzen, ...)
  // ---------------------------------------------------------------------------

  /**
   * Verschlüsselt einen vorzeichenbehafteten 8-Bit-Wert (-128 bis 127).
   *
   * Geeignet für: kleine Differenzwerte, Korrekturfaktoren
   * Beispiel: Genomics (Abweichungen), kleine Statistiken
   */
  encryptInt8(value: number, clientKey: TfheClientKey): Uint8Array {
    const enc = FheInt8.encrypt_with_client_key(value, clientKey);
    const bytes = enc.serialize();
    enc.free();
    return bytes;
  }

  /**
   * Verschlüsselt einen vorzeichenbehafteten 16-Bit-Wert (-32.768 bis 32.767).
   *
   * Geeignet für: Temperaturwerte, mittlere Differenzen
   * Beispiel: Statistics Service (Mittelwerte mit negativen Ausreißern)
   */
  encryptInt16(value: number, clientKey: TfheClientKey): Uint8Array {
    const enc = FheInt16.encrypt_with_client_key(value, clientKey);
    const bytes = enc.serialize();
    enc.free();
    return bytes;
  }

  /**
   * Verschlüsselt einen vorzeichenbehafteten 32-Bit-Wert (ca. ±2,1 Milliarden).
   *
   * Geeignet für: allgemeine Ganzzahlen mit Vorzeichen, Koordinaten
   * Beispiel: Statistics Service, Leaderboard-Differenzen
   */
  encryptInt32(value: number, clientKey: TfheClientKey): Uint8Array {
    const enc = FheInt32.encrypt_with_client_key(value, clientKey);
    const bytes = enc.serialize();
    enc.free();
    return bytes;
  }

  /**
   * Verschlüsselt einen vorzeichenbehafteten 64-Bit-Wert als BigInt.
   *
   * Geeignet für: sehr große vorzeichenbehaftete Werte
   * Beispiel: Program Execution (Register-Inhalte), große Statistiken
   * Hinweis: JavaScript number reicht nicht aus → BigInt verwenden
   */
  encryptInt64(value: bigint, clientKey: TfheClientKey): Uint8Array {
    const enc = FheInt64.encrypt_with_client_key(value, clientKey);
    const bytes = enc.serialize();
    enc.free();
    return bytes;
  }

  /**
   * Verschlüsselt einen booleschen Wert (true/false).
   *
   * Geeignet für: Flags, Berechtigungen, binäre Entscheidungen
   * Beispiel: Zugangskontrolle, Feature-Flags, Voting (ja/nein)
   */
  encryptBool(value: boolean, clientKey: TfheClientKey): Uint8Array {
    const enc = FheBool.encrypt_with_client_key(value, clientKey);
    const bytes = enc.serialize();
    enc.free();
    return bytes;
  }

  // ---------------------------------------------------------------------------
  // Entschlüsselung
  // Alle Funktionen benötigen den Client-Key der niemals das Gerät verlässt.
  // ---------------------------------------------------------------------------

  /** Entschlüsselt einen FheUint8-Ciphertext → Zahl (0–255) */
  decryptUint8(bytes: Uint8Array, clientKey: TfheClientKey): number {
    const enc = FheUint8.deserialize(bytes);
    const value = enc.decrypt(clientKey);
    enc.free();
    return value;
  }

  /** Entschlüsselt einen FheUint16-Ciphertext → Zahl (0–65.535) */
  decryptUint16(bytes: Uint8Array, clientKey: TfheClientKey): number {
    const enc = FheUint16.deserialize(bytes);
    const value = enc.decrypt(clientKey);
    enc.free();
    return value;
  }

  /** Entschlüsselt einen FheUint32-Ciphertext → Zahl (0–4.294.967.295) */
  decryptUint32(bytes: Uint8Array, clientKey: TfheClientKey): number {
    const enc = FheUint32.deserialize(bytes);
    const value = enc.decrypt(clientKey);
    enc.free();
    return value;
  }

  /** Entschlüsselt einen FheUint64-Ciphertext → BigInt */
  decryptUint64(bytes: Uint8Array, clientKey: TfheClientKey): bigint {
    const enc = FheUint64.deserialize(bytes);
    const value = enc.decrypt(clientKey);
    enc.free();
    return value;
  }

  /** Entschlüsselt einen FheInt8-Ciphertext → Zahl (-128 bis 127) */
  decryptInt8(bytes: Uint8Array, clientKey: TfheClientKey): number {
    const enc = FheInt8.deserialize(bytes);
    const value = enc.decrypt(clientKey);
    enc.free();
    return value;
  }

  /** Entschlüsselt einen FheInt16-Ciphertext → Zahl (-32.768 bis 32.767) */
  decryptInt16(bytes: Uint8Array, clientKey: TfheClientKey): number {
    const enc = FheInt16.deserialize(bytes);
    const value = enc.decrypt(clientKey);
    enc.free();
    return value;
  }

  /** Entschlüsselt einen FheInt32-Ciphertext → Zahl (ca. ±2,1 Milliarden) */
  decryptInt32(bytes: Uint8Array, clientKey: TfheClientKey): number {
    const enc = FheInt32.deserialize(bytes);
    const value = enc.decrypt(clientKey);
    enc.free();
    return value;
  }

  /** Entschlüsselt einen FheInt64-Ciphertext → BigInt */
  decryptInt64(bytes: Uint8Array, clientKey: TfheClientKey): bigint {
    const enc = FheInt64.deserialize(bytes);
    const value = enc.decrypt(clientKey);
    enc.free();
    return value;
  }

  /** Entschlüsselt einen FheBool-Ciphertext → boolean */
  decryptBool(bytes: Uint8Array, clientKey: TfheClientKey): boolean {
    const enc = FheBool.deserialize(bytes);
    const value = enc.decrypt(clientKey);
    enc.free();
    return value;
  }

  // ---------------------------------------------------------------------------
  // Hilfsfunktionen – Base64-Kodierung für HTTP-Transport
  // Base64 ist KEINE Verschlüsselung, nur eine Textkodierung für Binärdaten.
  // Der eigentliche Ciphertext (FHE) steckt in den kodierten Bytes drin.
  // ---------------------------------------------------------------------------

  /** Kodiert Bytes als Base64-String für den HTTP-Transport. Chunk-sicher für große Puffer. */
  toBase64(bytes: Uint8Array): string {
    let binary = '';
    const chunkSize = 8192;
    for (let i = 0; i < bytes.length; i += chunkSize) {
      binary += String.fromCharCode(...bytes.subarray(i, i + chunkSize));
    }
    return btoa(binary);
  }

  /** Dekodiert einen Base64-String zurück zu Bytes. */
  fromBase64(base64: string): Uint8Array {
    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
  }
}

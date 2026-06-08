import { Injectable, inject } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable, map } from 'rxjs';
import { SERVICE_URLS } from './service-urls';

/**
 * HTTP-Schnittstelle zum Service 01 (Encrypted Key-Value Store).
 *
 * Alle verschlüsselten Felder reisen als **Base64-Strings über JSON**:
 * - Ein vollständiger verschlüsselter String → `string[]` (ein Element pro
 *   Zeichen, jedes ist ein bincode-`FheUint8`).
 * - Ein skalarer Ciphertext (z.B. `FheBool` für `exists`) → ein einzelner
 *   `string` (bincode-Bytes als Base64).
 *
 * Der ServerKey wird einmal beim Anlegen der Session hochgeladen (groß, lohnt
 * sich nicht pro Request) und der Server hängt ihn an die zurückgegebene
 * `session_id`. Geht die Session am Server verloren (Restart, TTL), liefert
 * der nächste Request 401 und der Client muss die Session neu öffnen.
 */
@Injectable({ providedIn: 'root' })
export class KeyValueStoreApiService {
  /**
   * Relativer URL-Prefix. Der Angular-Dev-Proxy strippt `/kv` lokal,
   * im Cluster macht Traefik dasselbe — der Service selbst kennt diesen
   * Prefix nicht.
   */
  private readonly base = SERVICE_URLS.keyValueStore.path;

  private readonly http = inject(HttpClient);

  /**
   * Öffnet eine neue Session. Der Server merkt sich den dekomprimierten
   * ServerKey unter der zurückgegebenen `session_id`.
   *
   * @param serverKeyB64 Base64-kodierter bincode-`CompressedServerKey`.
   */
  createSession(serverKeyB64: string): Observable<string> {
    return this.http
      .post<{ session_id: string }>(`${this.base}/session`, {
        server_key: serverKeyB64,
      })
      .pipe(map((res) => res.session_id));
  }

  /**
   * Speichert ein verschlüsseltes Key/Value-Paar mit optionaler TTL
   * (Klartext-Sekunden). Ohne TTL gilt der Server-Default (siehe ENV
   * `TTL_MINUTES`).
   */
  put(
    sessionId: string,
    keyChunks: string[],
    valueChunks: string[],
    ttlSeconds?: number,
  ): Observable<void> {
    return this.http
      .post<unknown>(`${this.base}/entry`, {
        session_id: sessionId,
        key: keyChunks,
        value: valueChunks,
        ttl_seconds: ttlSeconds,
      })
      .pipe(map(() => undefined));
  }

  /**
   * Holt den verschlüsselten Wert, dessen verschlüsselter Schlüssel zum
   * übergebenen passt. Liefert 404, wenn die Session leer ist.
   */
  get(sessionId: string, keyChunks: string[]): Observable<string[]> {
    return this.http
      .post<{ value: string[] }>(`${this.base}/entry/get`, {
        session_id: sessionId,
        key: keyChunks,
      })
      .pipe(map((res) => res.value));
  }

  /**
   * Liefert einen einzelnen verschlüsselten `FheBool` (Base64) zurück — der
   * Client entschlüsselt mit dem ClientKey, ob der Schlüssel existiert.
   */
  exists(sessionId: string, keyChunks: string[]): Observable<string> {
    return this.http
      .post<{ exists: string }>(`${this.base}/entry/exists`, {
        session_id: sessionId,
        key: keyChunks,
      })
      .pipe(map((res) => res.exists));
  }

  /** Löscht alle Einträge dieser Session. */
  clear(sessionId: string): Observable<void> {
    return this.http
      .post<unknown>(`${this.base}/clear`, { session_id: sessionId })
      .pipe(map(() => undefined));
  }
}

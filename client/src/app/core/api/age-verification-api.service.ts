import { Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable, map, switchMap } from 'rxjs';
import { SERVICE_URLS } from './service-urls';

interface SetupRequest {
  server_key: string;
}

interface SetupResponse {
  session_id: string;
}

interface VerifyRequest {
  encrypted_age: string;
}

interface VerifyResponse {
  is_adult: string;
}

@Injectable({ providedIn: 'root' })
export class AgeVerificationApiService {
  private readonly baseUrl = SERVICE_URLS.ageVerification.path;

  constructor(private http: HttpClient) {}

  /**
   * Phase 1: Lädt den ServerKey einmalig hoch und gibt die session_id zurück.
   * Der Server dekomprimiert und cacht den Key – folgende Requests sind ~88 KB.
   */
  setupSession(serverKeyB64: string): Observable<string> {
    const body: SetupRequest = { server_key: serverKeyB64 };
    return this.http
      .post<SetupResponse>(`${this.baseUrl}/session`, body)
      .pipe(map((res) => res.session_id));
  }

  /**
   * Phase 2: Sendet nur encrypted_age gegen eine bestehende Session.
   * Gibt die Base64-kodierten Bytes des verschlüsselten Ergebnisses zurück.
   */
  verify(sessionId: string, encryptedAgeB64: string): Observable<string> {
    const body: VerifyRequest = { encrypted_age: encryptedAgeB64 };
    return this.http
      .post<VerifyResponse>(`${this.baseUrl}/verify/${sessionId}`, body)
      .pipe(map((res) => res.is_adult));
  }

  /**
   * Phase 3 (optional): Löscht die Session und gibt den serverseitigen RAM frei.
   */
  deleteSession(sessionId: string): Observable<void> {
    return this.http
      .delete<void>(`${this.baseUrl}/session/${sessionId}`);
  }

  /**
   * Kombinierter Flow: Setup → Verify in einem Schritt.
   * Nützlich wenn nur eine einzige Verifikation pro Schlüsselpaar stattfindet.
   */
  setupAndVerify(serverKeyB64: string, encryptedAgeB64: string): Observable<{ sessionId: string; isAdultB64: string }> {
    return this.setupSession(serverKeyB64).pipe(
      switchMap((sessionId) =>
        this.verify(sessionId, encryptedAgeB64).pipe(
          map((isAdultB64) => ({ sessionId, isAdultB64 }))
        )
      )
    );
  }
}
import { Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable, map } from 'rxjs';
import { SERVICE_URLS } from './service-urls';

interface AgeRequest {
  encrypted_age: string;
  server_key: string;
}

interface AgeResponse {
  is_adult: string;
}

@Injectable({ providedIn: 'root' })
export class AgeVerificationApiService {
  // Relative URL → wird vom Proxy (proxy.conf.js) an lokal oder remote weitergeleitet
  private readonly url = SERVICE_URLS.ageVerification.path;

  constructor(private http: HttpClient) {}

  /**
   * Sendet verschlüsseltes Alter + Server-Key ans Backend.
   * Gibt die Base64-kodierten Bytes des verschlüsselten Ergebnisses zurück.
   */
  verify(encryptedAgeB64: string, serverKeyB64: string): Observable<string> {
    const body: AgeRequest = {
      encrypted_age: encryptedAgeB64,
      server_key: serverKeyB64,
    };
    return this.http
      .post<AgeResponse>(this.url, body)
      .pipe(map((res) => res.is_adult));
  }
}

import { Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable, map } from 'rxjs';

const API_URL = '/age-verification';

interface AgeRequest {
  encrypted_age: string;
  server_key: string;
}

interface AgeResponse {
  is_adult: string;
}

@Injectable({ providedIn: 'root' })
export class AgeVerificationApiService {
  constructor(private http: HttpClient) {}

  /**
   * Sends encrypted age and server key to the backend.
   * Returns the encrypted FheBool result as bytes.
   */
  verify(encryptedAgeB64: string, serverKeyB64: string): Observable<string> {
    const body: AgeRequest = {
      encrypted_age: encryptedAgeB64,
      server_key: serverKeyB64,
    };
    return this.http
      .post<AgeResponse>(API_URL, body)
      .pipe(map((res) => res.is_adult));
  }
}

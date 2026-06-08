import { Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable, map } from 'rxjs';
import { SERVICE_URLS } from './service-urls';

interface CreateSessionRequest {
  server_key: [number];
}

interface CreateSessionResponse {
  is_adult: string;
}

@Injectable({ providedIn: 'root' })
export class AgeVerificationApiService {
  // Relative URL → wird vom Proxy (proxy.conf.js) an lokal oder remote weitergeleitet
  private readonly url = SERVICE_URLS.ageVerification.path;

  constructor(private http: HttpClient) {}


  verify(encryptedAgeB64: string, compressed_server_key: [number]): Observable<string> {
    const body: CreateSessionRequest = {
      server_key: compressed_server_key,
    };
    return this.http.post<CreateSessionResponse>(this.url, body).pipe(map((res) => res.is_adult));
  }
}

import { Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';
import { SERVICE_URLS } from './service-urls';

interface StatisticsRequest {
  encrypted_list: string[];
  server_key: string;
}

export interface StatisticsResult {
  sum: string;
  count: number;
  min: string;
  max: string;
  average: string;
  median: string;
}

@Injectable({ providedIn: 'root' })
export class StatisticsApiService {
  private readonly url = SERVICE_URLS.statistics.path;

  constructor(private http: HttpClient) {}

  /**
   * Sendet eine verschlüsselte Ganzzahlen-Liste + Server-Key ans Backend.
   * Der Server berechnet alle Statistiken homomorph und gibt verschlüsselte
   * Ergebnisse zurück.
   */
  compute(encryptedList: string[], serverKeyB64: string): Observable<StatisticsResult> {
    const body: StatisticsRequest = {
      encrypted_list: encryptedList,
      server_key: serverKeyB64,
    };
    return this.http.post<StatisticsResult>(this.url, body);
  }
}

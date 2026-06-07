import { Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';
import { SERVICE_URLS } from './service-urls';

interface StatisticsRequest {
  encrypted_list: string[];
  server_key: string;
  /** Bitbreite der Eingabewerte: 8, 16 oder 32. Wird vom Client auto-erkannt. */
  bit_width: 8 | 16 | 32;
}

export interface StatisticsResult {
  sum: string;
  count: number;
  min: string;
  max: string;
  average: string;
  median: string;
  bit_width: 8 | 16 | 32;
}

/**
 * HTTP-Client für den Statistics-Service (Service 05).
 * Sendet verschlüsselte Ganzzahlen-Listen an das Backend und empfängt
 * homomorph berechnete, verschlüsselte Statistiken.
 */
@Injectable({ providedIn: 'root' })
export class StatisticsApiService {
  private readonly serviceUrl = SERVICE_URLS.statistics.path;

  constructor(private readonly httpClient: HttpClient) {}

  /**
   * Sendet eine verschlüsselte Ganzzahlen-Liste + Server-Key ans Backend.
   * `bitWidth` gibt an, mit welchem FHE-Typ die Werte verschlüsselt wurden —
   * der Server wählt darauf basierend den passenden generischen Code-Pfad.
   */
  compute(
    encryptedNumberList: string[],
    serverKeyBase64: string,
    bitWidth: 8 | 16 | 32,
  ): Observable<StatisticsResult> {
    const requestBody: StatisticsRequest = {
      encrypted_list: encryptedNumberList,
      server_key: serverKeyBase64,
      bit_width: bitWidth,
    };
    return this.httpClient.post<StatisticsResult>(this.serviceUrl, requestBody);
  }
}

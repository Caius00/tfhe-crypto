import { Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';
import { SERVICE_URLS } from './service-urls';

interface SessionRequest {
  server_key: string;
}

interface SessionResponse {
  session_id: string;
}

interface StatisticsRequest {
  session_id: string;
  encrypted_list: string[];
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

@Injectable({ providedIn: 'root' })
export class StatisticsApiService {
  private readonly serviceUrl = SERVICE_URLS.statistics.path;

  constructor(private readonly httpClient: HttpClient) {}

  createSession(serverKeyBase64: string): Observable<SessionResponse> {
    const body: SessionRequest = { server_key: serverKeyBase64 };
    return this.httpClient.post<SessionResponse>(`${this.serviceUrl}/session`, body);
  }

  compute(
    sessionId: string,
    encryptedNumberList: string[],
    bitWidth: 8 | 16 | 32,
  ): Observable<StatisticsResult> {
    const requestBody: StatisticsRequest = {
      session_id: sessionId,
      encrypted_list: encryptedNumberList,
      bit_width: bitWidth,
    };
    return this.httpClient.post<StatisticsResult>(this.serviceUrl, requestBody);
  }
}

import { Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';
import { SERVICE_URLS } from './service-urls';

const BASE = SERVICE_URLS.leaderboard.path;

interface CreateRequest {
  server_key: string;
  public_key: string;
}

interface SubmitRequest {
  player_key: string;
  encrypted_score: string;
  encrypted_id: string;
}

export interface EntryDto {
  encrypted_score: string;
  encrypted_id: string;
}

export interface RosterEntryDto {
  player_key: string;
  encrypted_id: string;
}

export interface EntriesResponse {
  entries: EntryDto[];
  roster: RosterEntryDto[];
}

@Injectable({ providedIn: 'root' })
export class LeaderboardApiService {
  constructor(private http: HttpClient) {}

  create(serverKeyB64: string, publicKeyB64: string): Observable<{ code: string }> {
    const body: CreateRequest = { server_key: serverKeyB64, public_key: publicKeyB64 };
    return this.http.post<{ code: string }>(`${BASE}/create`, body);
  }

  getPublicKey(code: string): Observable<{ public_key: string }> {
    return this.http.get<{ public_key: string }>(`${BASE}/${code}/public-key`);
  }

  submit(code: string, playerKey: string, encryptedScoreB64: string, encryptedIdB64: string): Observable<void> {
    const body: SubmitRequest = {
      player_key: playerKey,
      encrypted_score: encryptedScoreB64,
      encrypted_id: encryptedIdB64,
    };
    return this.http.post<void>(`${BASE}/${code}/submit`, body);
  }

  getEntries(code: string): Observable<EntriesResponse> {
    return this.http.get<EntriesResponse>(`${BASE}/${code}/entries`);
  }
}

import { Injectable, inject } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';

@Injectable({ providedIn: 'root' })
export class VotingService {
  private http = inject(HttpClient);
  private baseUrl = 'http://localhost:8080';

  createSession(
    creatorId: string,
    serverKey: string,
    questions: any[],
  ): Observable<{ session_id: string }> {
    return this.http.post<{ session_id: string }>(`${this.baseUrl}/session`, {
      creator_id: creatorId,
      server_key: serverKey,
      questions,
    });
  }
  joinSession(sessionId: string, participantId: string): Observable<{ status: string }> {
    return this.http.post<{ status: string }>(`${this.baseUrl}/join`, {
      session_id: sessionId,
      participant_id: participantId,
    });
  }

  getPending(sessionId: string, creatorId: string): Observable<string[]> {
    return this.http.get<string[]>(`${this.baseUrl}/pending/${sessionId}/${creatorId}`);
  }

  approveParticipant(
    sessionId: string,
    creatorId: string,
    participantId: string,
    approved: boolean,
  ): Observable<{ status: string }> {
    return this.http.post<{ status: string }>(`${this.baseUrl}/approve`, {
      session_id: sessionId,
      creator_id: creatorId,
      participant_id: participantId,
      approved,
    });
  }

  submitVote(
    sessionId: string,
    participantId: string,
    encryptedVotes: string[],
  ): Observable<{ status: string }> {
    return this.http.post<{ status: string }>(`${this.baseUrl}/vote`, {
      session_id: sessionId,
      participant_id: participantId,
      encrypted_votes: encryptedVotes,
    });
  }

  getResults(
    sessionId: string,
    creatorId: string,
  ): Observable<{ encrypted_results: string[]; ready: boolean }> {
    return this.http.get<{ encrypted_results: string[]; ready: boolean }>(
      `${this.baseUrl}/results/${sessionId}/${creatorId}`,
    );
  }
}

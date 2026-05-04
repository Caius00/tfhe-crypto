// src/app/voting/voting.service.ts
import { Injectable, inject } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Question } from './voting.types';
import { Observable } from 'rxjs';

export interface CreateSessionResponse {
  session_id: string;
}

export interface SessionMetadata {
  session_id: string;
  creator_id: string;
  public_key?: string | null; // Base64
  server_key?: string; // Base64
  questions: Question[];
}

export interface JoinResponse {
  status: string;
}

export interface PendingEntry {
  participant_id: string;
  enc_name_chunks: string[]; // array of Base64 chunks
}

export interface ResultsResponse {
  encrypted_results: string[]; // Base64 per aggregated ciphertext
  ready: boolean;
}

@Injectable({ providedIn: 'root' })
export class VotingService {
  private http = inject(HttpClient);
  private baseUrl = 'http://localhost:8080';

  /**
   * Create a new session.
   * - serverKey: Base64 (compressed server key)
   * - publicKey: Base64 (optional) — public/encryption representation for participants
   */
  createSession(
    creatorId: string,
    serverKey: string,
    publicKey: string | null,
    questions: Question[]
  ): Observable<CreateSessionResponse> {
    return this.http.post<CreateSessionResponse>(
      `${this.baseUrl}/session`,
      { creator_id: creatorId, server_key: serverKey, public_key: publicKey, questions }
    );
  }

  storeClientKey(sessionId: string, clientKeyB64: string): Observable<any> {
    return this.http.post(`${this.baseUrl}/store-key`, {
      session_id: sessionId,
      client_key: clientKeyB64
    });
  }

  loadClientKey(sessionId: string): Observable<{ client_key: string }> {
    return this.http.get<{ client_key: string }>(`${this.baseUrl}/load-key/${sessionId}`);
  }

  /**
   * Participant joins a session.
   * - encNameChunks: array of Base64 strings (chunks) representing encrypted name
   */
  joinSession(
    sessionId: string,
    participantId: string,
    encNameChunks: string[] // Base64 chunks
  ): Observable<JoinResponse> {
    return this.http.post<JoinResponse>(
      `${this.baseUrl}/join`,
      { session_id: sessionId, participant_id: participantId, enc_name_chunks: encNameChunks }
    );
  }

  getStatus(sessionId: string, participantId: string) {
    return this.http.get<{ status: string }>(
      `${this.baseUrl}/status/${sessionId}/${participantId}`
    );
  }

  /**
   * Get pending participants for a session (creator only).
   * Returns array of { participant_id, enc_name_chunks }.
   */
  getPending(sessionId: string, creatorId: string) {
    return this.http.get<PendingEntry[]>(
      `${this.baseUrl}/pending/${sessionId}/${creatorId}`
    );
  }

  approveParticipant(sessionId: string, creatorId: string, participantId: string, approved: boolean) {
    return this.http.post<{ status: string }>(
      `${this.baseUrl}/approve`,
      { session_id: sessionId, creator_id: creatorId, participant_id: participantId, approved }
    );
  }

  /**
   * Submit encrypted votes.
   * encryptedVotes: array of Base64 strings (one per question, or per question-encoding)
   */
  submitVote(sessionId: string, participantId: string, encryptedVotes: string[]) {
    return this.http.post<{ status: string }>(
      `${this.baseUrl}/vote`,
      { session_id: sessionId, participant_id: participantId, encrypted_votes: encryptedVotes }
    );
  }

  /**
   * Get session metadata (questions, public_key, etc.)
   */
  getSession(sessionId: string) {
    return this.http.get<SessionMetadata>(
      `${this.baseUrl}/session/${sessionId}`
    );
  }

  getResults(sessionId: string, creatorId: string) {
    return this.http.get<ResultsResponse>(
      `${this.baseUrl}/results/${sessionId}/${creatorId}`
    );
  }

  finalizeSession(sessionId: string, creatorId: string) {
    return this.http.post(
      `${this.baseUrl}/finalize/${sessionId}/${creatorId}`,
      {}
    );
  }
}

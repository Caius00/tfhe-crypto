import { Component, inject, signal } from '@angular/core';
import { JsonPipe } from '@angular/common';
import { FormsModule } from '@angular/forms'; // <--- ADD THIS
import { HttpClient } from '@angular/common/http';
import { TfheService } from '../../core/crypto/tfhe.service';
import { SERVICE_URLS } from '../../core/api/service-urls';
import { firstValueFrom } from 'rxjs';
import { KeyPair } from '../../core/crypto/key-pair.model';
import { CompressedFheUint8, TfheClientKey } from 'tfhe';
import { Serializer } from '@angular/compiler';

interface SessionResponse {
  message: string;
}

interface ValueResponse {
  value: number[];
}

interface MessageResponse {
  message: string;
}

@Component({
  selector: 'app-key-value-store',
  standalone: true,
  imports: [JsonPipe, FormsModule], // <--- ADD THIS
  templateUrl: './key-value-store.component.html',
})
export class KeyValueStoreComponent {
  private readonly tfheService = inject(TfheService);
  private readonly http = inject(HttpClient);
  private readonly baseUrl = SERVICE_URLS.keyValueStore.path;

  // State signals
  readonly response = signal<unknown>(null);
  readonly message = signal<string | null>(null);
  readonly error = signal<string | null>(null);
  readonly sessionId = signal<string | null>(null);

  // <--- ADD INPUT SIGNALS HERE
  readonly key = signal('');
  readonly value = signal('');

  private keyPair: KeyPair | null = null;

  readonly actions = [
    { label: 'Create Keypair', handler: () => this.createKeypair() },
    { label: 'Create Session', handler: () => this.createSession() },
    { label: 'Put', handler: () => this.put() }, // Now wired up
    { label: 'Get', handler: () => this.get() },
    { label: 'Exists', handler: () => this.exists() },
    { label: 'Delete', handler: () => this.delete() },
  ];

  async createKeypair(): Promise<void> {
    await this.tfheService.ensureInitialized();
    this.keyPair = this.tfheService.generateKeyPair();

    this.message.set('Key pair generated successfully.');
    this.error.set(null);
    this.response.set(null);
  }

  async createSession(): Promise<void> {
    if (this.keyPair == null) {
      // tell frontend that keys have to be generated first
      this.error.set('Please generate a Key Pair first.');
      this.message.set(null);
      this.response.set(null);
      return;
    }

    try {
      // Type the HTTP response so TS knows about `message`
      const res = await firstValueFrom(
        this.http.post<SessionResponse>(`${this.baseUrl}/session`, {
          server_key: Array.from(this.keyPair.serverKeyBytes),
        }),
      );

      // show frontend success message and display sessionId
      this.sessionId.set(res.message);
      this.message.set(`Session created successfully! ID: ${res.message}`);
      this.error.set(null);
      this.response.set(res);
    } catch (err) {
      this.error.set('Failed to create session. Is the server running?');
      this.message.set(null);
      this.response.set(err);
    }
  }

  async put(): Promise<void> {
    const currentKey = this.key().trim();
    const currentValue = this.value().trim();

    if (this.keyPair == null) {
      this.error.set('Please generate a Key Pair first.');
      return;
    }
    const keyPair = this.keyPair;
    const activeSessionId = this.sessionId();
    if (!activeSessionId) {
      this.error.set('Please create a session first.');
      return;
    }
    if (!currentKey || !currentValue) {
      this.error.set('Both Key and Value are required.');
      return;
    }

    let enc_key = encryptString(currentKey, keyPair.clientKey);
    let enc_value = encryptString(currentValue, keyPair.clientKey);

    try {
      // 3. Make the request (Adjust payload to match your actual API)
      const res = await firstValueFrom(
        this.http.post(`${this.baseUrl}/entry`, {
          key: enc_key,
          value: enc_value,
          session_id: activeSessionId,
        }),
      );

      // 4. Handle Success
      this.message.set(`Successfully put "${currentKey}"`);
      this.error.set(null);
      this.response.set(res);

      // Clear the inputs after successful put
      this.key.set('');
      this.value.set('');
    } catch (err) {
      // 5. Handle Error
      this.error.set('Failed to put data.');
      this.message.set(null);
      this.response.set(err);
    }
  }

  async get(): Promise<void> {
    const currentKey = this.key().trim();

    if (this.keyPair == null) {
      this.error.set('Please generate a Key Pair first.');
      return;
    }
    const keyPair = this.keyPair;
    const activeSessionId = this.sessionId();
    if (!activeSessionId) {
      this.error.set('Please create a session first.');
      return;
    }
    if (!currentKey) {
      this.error.set('Key is required.');
      return;
    }

    let enc_key = encryptString(currentKey, keyPair.clientKey);

    try {
      // 3. Make the GET request
      const res = await firstValueFrom(
        this.http.get<ValueResponse>(`${this.baseUrl}/entry`, {
          params: {
            key: currentKey,
            session_id: activeSessionId,
          },
        }),
      );

      // 4. Handle Success
      this.message.set(`Successfully retrieved "${currentKey}"`);
      this.error.set(null);
      this.response.set(res);

      // Clear the inputs after successful put
      this.key.set('');
      this.value.set('');
    } catch (err) {
      // 5. Handle Error
      this.error.set('Failed to get data.');
      this.message.set(null);
      this.response.set(err);
    }
  }
  async exists(): Promise<void> {
    const currentKey = this.key().trim();

    if (this.keyPair == null) {
      this.error.set('Please generate a Key Pair first.');
      return;
    }
    const keyPair = this.keyPair;
    const activeSessionId = this.sessionId();
    if (!activeSessionId) {
      this.error.set('Please create a session first.');
      return;
    }
    if (!currentKey) {
      this.error.set('Key is required.');
      return;
    }

    let enc_key = encryptString(currentKey, keyPair.clientKey);

    try {
      // 3. Make the GET request
      const res = await firstValueFrom(
        this.http.get<ValueResponse>(`${this.baseUrl}/entry/exists`, {
          params: {
            key: currentKey,
            session_id: activeSessionId,
          },
        }),
      );

      // 4. Handle Success
      this.message.set(`Successfully retrieved "${currentKey}"`);
      this.error.set(null);
      this.response.set(res);

      // Clear the inputs after successful put
      this.key.set('');
      this.value.set('');
    } catch (err) {
      // 5. Handle Error
      this.error.set('Failed to get data.');
      this.message.set(null);
      this.response.set(err);
    }
  }
  async delete(): Promise<void> {
    const currentKey = this.key().trim();

    if (this.keyPair == null) {
      this.error.set('Please generate a Key Pair first.');
      return;
    }
    const keyPair = this.keyPair;
    const activeSessionId = this.sessionId();
    if (!activeSessionId) {
      this.error.set('Please create a session first.');
      return;
    }
    if (!currentKey) {
      this.error.set('Key is required.');
      return;
    }

    let enc_key = encryptString(currentKey, keyPair.clientKey);

    try {
      // 3. Make the GET request
      const res = await firstValueFrom(
        this.http.delete(`${this.baseUrl}/entry`, {
          params: {
            key: currentKey,
            session_id: activeSessionId,
          },
        }),
      );

      // 4. Handle Success
      this.message.set(`Successfully retrieved "${currentKey}"`);
      this.error.set(null);
      this.response.set(res);

      // Clear the inputs after successful put
      this.key.set('');
      this.value.set('');
    } catch (err) {
      // 5. Handle Error
      this.error.set('Failed to get data.');
      this.message.set(null);
      this.response.set(err);
    }
  }
}

function encryptString(str: string, clientKey: TfheClientKey) {
  // TODO()
}

function decryptString(encrypted: string, clientKey: TfheClientKey): string {
  // TODO()
  return encrypted
}

import { Component, inject } from '@angular/core';
import { JsonPipe } from '@angular/common';
import { HttpClient } from '@angular/common/http';
import { TfheService } from '../../core/crypto/tfhe.service';
import { SERVICE_URLS } from '../../core/api/service-urls';
import { firstValueFrom } from 'rxjs';

@Component({
  selector: 'app-key-value-store',
  standalone: true,
  imports: [JsonPipe],
  templateUrl: './key-value-store.component.html',
})
export class KeyValueStoreComponent {
  private readonly tfheService = inject(TfheService);
  private readonly http = inject(HttpClient);
  private baseUrl = SERVICE_URLS.keyValueStore.path;

  response: unknown;

  readonly actions = [
    { label: 'Create Session', handler: () => this.createSession() },
    { label: 'Put', handler: () => this.put() },
    { label: 'Get', handler: () => this.get() },
    { label: 'Exists', handler: () => this.exists() },
    { label: 'Delete', handler: () => this.delete() },
    { label: 'Clear', handler: () => this.clear() },
  ];

  async createSession(): Promise<void> {
    await this.tfheService.ensureInitialized();

    const keyPair = this.tfheService.generateKeyPair();

    this.response = await firstValueFrom(
      this.http.post(`${this.baseUrl}/session`, {
        compressed_server_key: Array.from(keyPair.serverKeyBytes),
      }),
    );
  }

  put(): void {}
  get(): void {}
  exists(): void {}
  delete(): void {}
  clear(): void {}
}



import { Component, OnDestroy, signal } from '@angular/core';
import { TfheService } from '../../core/crypto/tfhe.service';
import { LeaderboardApiService } from '../../core/api/leaderboard-api.service';
import { KeyPair } from '../../core/crypto/key-pair.model';
import { SpinnerComponent } from '../../shared/components/spinner/spinner.component';
import { ButtonComponent } from '../../shared/components/button/button.component';
import { AlertComponent } from '../../shared/components/alert/alert.component';
import { LeaderboardLandingComponent } from './leaderboard-landing.component';
import { LeaderboardCreatorComponent } from './leaderboard-creator.component';
import { DecryptedEntry } from './models/decrypted-entry.model';
import { LeaderboardPlayerComponent } from './leaderboard-player.component';

type View = 'landing' | 'creating' | 'creator' | 'player' | 'error';

const POLL_INTERVAL_MS = 8_000;

@Component({
  selector: 'app-leaderboard',
  imports: [
    SpinnerComponent,
    ButtonComponent,
    AlertComponent,
    LeaderboardLandingComponent,
    LeaderboardCreatorComponent,
    LeaderboardPlayerComponent,
  ],
  templateUrl: './leaderboard.component.html',
  styleUrl: './leaderboard.component.css',
})
export class LeaderboardComponent implements OnDestroy {
  view = signal<View>('landing');
  errorMessage = signal('');
  roomCode = signal('');

  // Creator state
  creatorEntries = signal<DecryptedEntry[]>([]);
  creatorLoading = signal(false);
  lastUpdated = signal<Date | null>(null);
  private keyPair: KeyPair | null = null;
  private pollTimer: ReturnType<typeof setInterval> | null = null;

  // Player state
  playerId = signal('');
  private publicKeyBytes: Uint8Array | null = null;

  constructor(
    private tfhe: TfheService,
    private api: LeaderboardApiService,
  ) {}

  ngOnDestroy(): void {
    this.stopPolling();
  }

  // ---------------------------------------------------------------------------
  // Creator flow
  // ---------------------------------------------------------------------------

  async onCreateRoom(): Promise<void> {
    this.view.set('creating');
    await new Promise((r) => setTimeout(r, 50));

    try {
      await this.tfhe.ensureInitialized();
      this.keyPair = this.tfhe.generateKeyPair();

      const publicKeyBytes = this.tfhe.generatePublicKey(this.keyPair.clientKey);
      const serverKeyB64 = this.tfhe.toBase64(this.keyPair.serverKeyBytes);
      const publicKeyB64 = this.tfhe.toBase64(publicKeyBytes);

      this.api.create(serverKeyB64, publicKeyB64).subscribe({
        next: (res) => {
          this.roomCode.set(res.code);
          this.view.set('creator');
          this.fetchLeaderboard();
          this.startPolling();
        },
        error: (err) => this.setError(`Raum konnte nicht erstellt werden: ${err.message}`),
      });
    } catch (e) {
      console.error('Key generation error:', e);
      const msg = e instanceof Error ? e.message : String(e);
      this.setError(`Fehler bei der Schlüsselgenerierung: ${msg}`);
    }
  }

  fetchLeaderboard(): void {
    if (!this.keyPair) return;
    this.creatorLoading.set(true);

    this.api.getEntries(this.roomCode()).subscribe({
      next: (res) => {
        const entries: DecryptedEntry[] = res.entries.map((e, i) => {
          const scoreBytes = this.tfhe.fromBase64(e.encrypted_score);
          const idBytes = this.tfhe.fromBase64(e.encrypted_id);
          const score = this.tfhe.decryptUint16(scoreBytes, this.keyPair!.clientKey);
          const id = this.tfhe.decryptUint8(idBytes, this.keyPair!.clientKey);
          return {
            rank: i + 1,
            score,
            playerId: id.toString(16).padStart(2, '0').toUpperCase(),
          };
        });
        this.creatorEntries.set(entries);
        this.lastUpdated.set(new Date());
        this.creatorLoading.set(false);
      },
      error: () => {
        this.creatorLoading.set(false);
      },
    });
  }

  private startPolling(): void {
    this.stopPolling();
    this.pollTimer = setInterval(() => this.fetchLeaderboard(), POLL_INTERVAL_MS);
  }

  private stopPolling(): void {
    if (this.pollTimer !== null) {
      clearInterval(this.pollTimer);
      this.pollTimer = null;
    }
  }

  // ---------------------------------------------------------------------------
  // Player flow
  // ---------------------------------------------------------------------------

  onJoinRoom(code: string): void {
    this.roomCode.set(code);
    const STORAGE_KEY = 'lb_player_id';
    const stored = localStorage.getItem(STORAGE_KEY);
    const hexId = stored ?? (() => {
      const id = Math.floor(Math.random() * 0xff).toString(16).padStart(2, '0').toUpperCase();
      localStorage.setItem(STORAGE_KEY, id);
      return id;
    })();
    this.playerId.set(hexId);

    this.api.getPublicKey(code).subscribe({
      next: (res) => {
        this.publicKeyBytes = this.tfhe.fromBase64(res.public_key);
        this.view.set('player');
      },
      error: (err) => this.setError(`Raum nicht gefunden: ${err.message}`),
    });
  }

  // Called automatically after every game over
  async onSubmitScore(score: number): Promise<void> {
    if (!this.publicKeyBytes) return;

    try {
      await this.tfhe.ensureInitialized();
      const playerId = parseInt(this.playerId(), 16);
      const { encryptedScore, encryptedId } = this.tfhe.encryptScoreAndId(
        score,
        playerId,
        this.publicKeyBytes,
      );

      this.api
        .submit(this.roomCode(), this.playerId(), this.tfhe.toBase64(encryptedScore), this.tfhe.toBase64(encryptedId))
        .subscribe({
          error: (err) => console.error('Submit error:', err),
        });
    } catch (e) {
      console.error('Encrypt error:', e);
    }
  }

  // ---------------------------------------------------------------------------
  // Helpers
  // ---------------------------------------------------------------------------

  goHome(): void {
    this.stopPolling();
    this.view.set('landing');
    this.errorMessage.set('');
    this.roomCode.set('');
    this.keyPair = null;
    this.publicKeyBytes = null;
    this.creatorEntries.set([]);
    this.lastUpdated.set(null);
  }

  private setError(msg: string): void {
    this.stopPolling();
    this.errorMessage.set(msg);
    this.view.set('error');
  }
}

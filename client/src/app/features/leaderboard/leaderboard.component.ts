import { Component, OnDestroy, signal } from '@angular/core';
import { TfheService } from '../../core/crypto/tfhe.service';
import { LeaderboardApiService, RosterEntryDto } from '../../core/api/leaderboard-api.service';
import { KeyPair } from '../../core/crypto/key-pair.model';
import { SpinnerComponent } from '../../shared/components/spinner/spinner.component';
import { ButtonComponent } from '../../shared/components/button/button.component';
import { AlertComponent } from '../../shared/components/alert/alert.component';
import { LeaderboardLandingComponent } from './leaderboard-landing.component';
import { LeaderboardCreatorComponent } from './leaderboard-creator.component';
import { DecryptedEntry } from './models/decrypted-entry.model';
import { LeaderboardPlayerComponent } from './leaderboard-player.component';
import { LeaderboardNameInputComponent } from './leaderboard-name-input.component';
import { PlayerIdentity, PlayerIdentityService } from './player-identity.service';

type View = 'landing' | 'creating' | 'creator' | 'name-input' | 'player' | 'error';

const POLL_INTERVAL_MS = 8_000;
const UNKNOWN_NAME = '???';

@Component({
  selector: 'app-leaderboard',
  imports: [
    SpinnerComponent,
    ButtonComponent,
    AlertComponent,
    LeaderboardLandingComponent,
    LeaderboardCreatorComponent,
    LeaderboardPlayerComponent,
    LeaderboardNameInputComponent,
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
  playerName = signal('');
  private identity: PlayerIdentity | null = null;
  private publicKeyBytes: Uint8Array | null = null;

  constructor(
    private tfhe: TfheService,
    private api: LeaderboardApiService,
    private identityService: PlayerIdentityService,
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
        const nameByIdByte = this.buildNameMap(res.roster);
        const entries: DecryptedEntry[] = res.entries.map((e, i) => {
          const scoreBytes = this.tfhe.fromBase64(e.encrypted_score);
          const idBytes = this.tfhe.fromBase64(e.encrypted_id);
          const score = this.tfhe.decryptUint16(scoreBytes, this.keyPair!.clientKey);
          const idByte = this.tfhe.decryptUint8(idBytes, this.keyPair!.clientKey);
          return {
            rank: i + 1,
            score,
            playerName: nameByIdByte.get(idByte) ?? UNKNOWN_NAME,
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

  /**
   * Baut die Mapping-Tabelle "ID-Byte → Name" für den Creator.
   *
   * Wir entschlüsseln pro Roster-Eintrag das `encrypted_id` (FheUint8) und
   * speichern den Klartext-Namen aus `player_key` ("name:uuid"). Mit dieser
   * Tabelle kann jede sortierte Entry-Zeile ihren Namen finden.
   */
  private buildNameMap(roster: RosterEntryDto[]): Map<number, string> {
    if (!this.keyPair) return new Map();
    const result = new Map<number, string>();
    for (const r of roster) {
      try {
        const idBytes = this.tfhe.fromBase64(r.encrypted_id);
        const idByte = this.tfhe.decryptUint8(idBytes, this.keyPair.clientKey);
        result.set(idByte, this.identityService.nameFromPlayerKey(r.player_key));
      } catch (e) {
        console.warn('Roster decrypt failed for entry', r.player_key, e);
      }
    }
    return result;
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

  /**
   * Erster Schritt nach Code-Eingabe: Identität aus localStorage prüfen.
   * - Bereits bekannt → direkt in Player-View, kein Name-Dialog.
   * - Neu           → Name-Dialog einblenden, Identität wird erst danach erzeugt.
   */
  onJoinRoom(code: string): void {
    this.roomCode.set(code);
    const stored = this.identityService.get(code);
    if (stored) {
      this.activatePlayer(stored);
      return;
    }
    this.view.set('name-input');
  }

  /** Vom Name-Dialog: Name validiert, Identität anlegen und in Player-View wechseln. */
  onNameSubmitted(name: string): void {
    const identity = this.identityService.create(this.roomCode(), name);
    this.activatePlayer(identity);
  }

  private activatePlayer(identity: PlayerIdentity): void {
    this.identity = identity;
    this.playerName.set(identity.name);

    this.api.getPublicKey(this.roomCode()).subscribe({
      next: (res) => {
        this.publicKeyBytes = this.tfhe.fromBase64(res.public_key);
        this.view.set('player');
      },
      error: (err) => this.setError(`Raum nicht gefunden: ${err.message}`),
    });
  }

  // Called automatically after every game over
  async onSubmitScore(score: number): Promise<void> {
    if (!this.publicKeyBytes || !this.identity) return;

    try {
      await this.tfhe.ensureInitialized();
      const { encryptedScore, encryptedId } = this.tfhe.encryptScoreAndId(
        score,
        this.identity.encIdByte,
        this.publicKeyBytes,
      );

      this.api
        .submit(
          this.roomCode(),
          this.identityService.toPlayerKey(this.identity),
          this.tfhe.toBase64(encryptedScore),
          this.tfhe.toBase64(encryptedId),
        )
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
    this.identity = null;
    this.playerName.set('');
    this.creatorEntries.set([]);
    this.lastUpdated.set(null);
  }

  private setError(msg: string): void {
    this.stopPolling();
    this.errorMessage.set(msg);
    this.view.set('error');
  }
}

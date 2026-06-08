import { Injectable } from '@angular/core';

/**
 * Identität eines Spielers innerhalb eines konkreten Raums.
 *
 * - `name`     wird im UI angezeigt (Player-View, Creator-Leaderboard).
 * - `uuid`     ist intern und sorgt dafür, dass mehrere Spieler mit demselben
 *              Namen einen eindeutigen `player_key` zum Backend bekommen.
 * - `encIdByte` ist der `FheUint8`-Klartext-Wert (0..255), der pro Submit
 *              verschlüsselt im `encrypted_id`-Feld mitgeschickt wird. Der
 *              Creator entschlüsselt den Wert und mappt ihn auf den Namen.
 */
export interface PlayerIdentity {
  name: string;
  uuid: string;
  encIdByte: number;
}

const STORAGE_PREFIX = 'lb_player_';
const NAME_REGEX = /^[A-Za-z]+$/;
const NAME_MAX_LEN = 20;

@Injectable({ providedIn: 'root' })
export class PlayerIdentityService {
  /** Vorhandene Identität für einen Raum zurückgeben, oder null wenn neu. */
  get(roomCode: string): PlayerIdentity | null {
    const raw = localStorage.getItem(this.storageKey(roomCode));
    if (!raw) return null;
    try {
      const parsed = JSON.parse(raw) as PlayerIdentity;
      if (this.isValid(parsed)) return parsed;
      return null;
    } catch {
      return null;
    }
  }

  /**
   * Neue Identität anlegen und persistieren. Wirft, wenn der Name die
   * Validierung nicht besteht — das Aufrufer-Component prüft das vorher,
   * dies ist nur der Defense-in-Depth-Check.
   */
  create(roomCode: string, name: string): PlayerIdentity {
    const trimmed = name.trim();
    if (!this.isValidName(trimmed)) {
      throw new Error(`Ungültiger Name: ${name}`);
    }
    const identity: PlayerIdentity = {
      name: trimmed,
      uuid: crypto.randomUUID(),
      encIdByte: Math.floor(Math.random() * 256),
    };
    localStorage.setItem(this.storageKey(roomCode), JSON.stringify(identity));
    return identity;
  }

  /** Liefert das, was als `player_key` zum Backend geht. */
  toPlayerKey(identity: PlayerIdentity): string {
    return `${identity.name}:${identity.uuid}`;
  }

  /** Extrahiert den Namen aus einem Backend-`player_key` ("name:uuid"). */
  nameFromPlayerKey(playerKey: string): string {
    const idx = playerKey.indexOf(':');
    return idx >= 0 ? playerKey.slice(0, idx) : playerKey;
  }

  /** Validierungs-Helper — auch von der UI-Komponente live aufgerufen. */
  isValidName(name: string): boolean {
    return name.length >= 1 && name.length <= NAME_MAX_LEN && NAME_REGEX.test(name);
  }

  private storageKey(roomCode: string): string {
    return `${STORAGE_PREFIX}${roomCode}`;
  }

  private isValid(value: unknown): value is PlayerIdentity {
    if (typeof value !== 'object' || value === null) return false;
    const v = value as Record<string, unknown>;
    return (
      typeof v['name'] === 'string' &&
      typeof v['uuid'] === 'string' &&
      typeof v['encIdByte'] === 'number'
    );
  }
}

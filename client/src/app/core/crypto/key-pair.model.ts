// src/app/core/crypto/key-pair.model.ts
import { TfheClientKey } from 'tfhe';

export interface KeyPair {
  clientKey: TfheClientKey;
  serverKeyBytes: Uint8Array;
  publicKeyBytes: Uint8Array;  // optional = Uint8Array | undefined
}

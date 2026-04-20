import { TfheClientKey } from 'tfhe';

export interface KeyPair {
  clientKey: TfheClientKey;
  serverKeyBytes: Uint8Array;
}

import { inject, Injectable, signal } from '@angular/core';
import { FheUint8, TfheClientKey } from 'tfhe';
import { TfheService } from '../../../core/crypto/tfhe.service';

export interface EncryptedData {
    bytes: Uint8Array[];
    width: number;
    height: number;
}

export interface DecryptedData {
    bytes: Uint8Array;
    width: number;
    height: number;
}

@Injectable({
    providedIn: 'root'
})
export class ImageEncryptionService {
    tfheService = inject(TfheService);

    async encryptImage(image: DecryptedData, clientKey: TfheClientKey): Promise<EncryptedData> {
        const encryptedData: Uint8Array[] = [];
        const pixels: Uint8Array = image.bytes;
        for (const pixel of pixels) {
            const encrypted = await this.tfheService.encryptUint8(
                pixel, clientKey
            )
            encryptedData.push(encrypted);
        }
        return {
            bytes: encryptedData,
            width: image.width,
            height: image.height
        };
    }

    async decryptImage(image: EncryptedData, clientKey: TfheClientKey): Promise<DecryptedData> {
        const decryptedData: number[] = [];
        const pixels: Uint8Array[] = image.bytes;
        for (const pixel of pixels) {
            const decrypted = await this.tfheService.decryptUint8(
                pixel, clientKey
            )
            decryptedData.push(decrypted);
        }
        return {
            bytes: new Uint8Array(decryptedData),
            width: image.width,
            height: image.height
        };
    }
}
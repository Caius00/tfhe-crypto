import { Injectable, signal } from "@angular/core";

@Injectable({
    providedIn: 'root'
})
export class KeyStoreService {
    keyPair = signal<{clientKey: any; serverKeyBytes: Uint8Array; } | null>(null);

    addKey(keyPair: {clientKey: any; serverKeyBytes: Uint8Array; }) {
        this.keyPair.set(keyPair);
    }
}
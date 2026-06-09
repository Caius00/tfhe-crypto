import { Injectable, signal } from '@angular/core';

export interface ProcessedImage {
    previewUrl: string;
    bytes: Uint8Array;
    width: number;
    height: number;
}

@Injectable({
    providedIn: 'root'
})
export class ImageProcessingService {
    processedImage = signal<ProcessedImage | null>(null);
    setProcessedImage(image: ProcessedImage): void {
        this.processedImage.set(image);
    }
}
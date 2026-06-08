import { Injectable, signal } from "@angular/core";

@Injectable({
    providedIn: 'root'
})
export class ImageUrlService {
    createPreviewUrl(
        bytes: Uint8Array,
        width: number,
        height: number
    ): string {
        const canvas = document.createElement('canvas');
        canvas.width = width;
        canvas.height = height;

        const ctx = canvas.getContext('2d');

        if (!ctx) {
            throw new Error('Canvas context unavailable');
        }

        const imageData = ctx.createImageData(width, height);

        for (let i = 0; i < bytes.length; i++) {
            const gray = bytes[i];

            imageData.data[i * 4] = gray;
            imageData.data[i * 4 + 1] = gray;
            imageData.data[i * 4 + 2] = gray;
            imageData.data[i * 4 + 3] = 255;
        }

        ctx.putImageData(imageData, 0, 0);

        return canvas.toDataURL('image/png');
    }
}
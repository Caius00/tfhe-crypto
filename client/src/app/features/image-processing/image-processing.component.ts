import { Component, inject } from '@angular/core';
import { Router } from '@angular/router';
import { PageHeaderComponent } from '../../shared/components/page-header/page-header.component';
import { CardComponent } from '../../shared/components/card/card.component';
import { ButtonComponent } from '../../shared/components/button/button.component';
import { FileUploadComponent } from './components/file-upload/file-upload.component';
import { ImageFileService } from './services/image-file.service';
import { ImageProcessingService } from './services/image-processing.service';

@Component({
	selector: 'app-image-processing',
	standalone: true,
	imports: [
		PageHeaderComponent, 
		CardComponent, 
		ButtonComponent, 
		FileUploadComponent],
	templateUrl: './image-processing.component.html',
	styleUrl: './image-processing.component.css',
})
export class ImageProcessingComponent {
	public fileService = inject(ImageFileService);
	private processingService = inject(ImageProcessingService);
	private router = inject(Router);

	async goEditor(): Promise<void> {
		const file = this.fileService.imageFile();
		if (!file) return;
		const result = await this.convertToGrayscale(file);
		this.processingService.setProcessedImage(result);
		await this.router.navigateByUrl('/image-processing/editor');
	}

	async convertToGrayscale(file: File) : Promise<{
        previewUrl: string; 
        bytes: Uint8Array; 
        width: number; 
        height: number;
    }> {
        return new Promise((resolve, reject) => {
            const img = new Image();
    
            img.onload = () => {
                const canvas = document.createElement('canvas');
                const ctx = canvas.getContext('2d');
    
                if (!ctx) {
                    reject('Canvas nicht verfügbar!');
                    return;
                }

                canvas.width = img.width;
                canvas.height = img.height;

                ctx.drawImage(img, 0, 0);
    
                const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
                const pixels = imageData.data;
    
                const grayScaleBytes = new Uint8Array(
                    canvas.width * canvas.height
                )
    
                for (let i = 0, pixelIndex = 0; i < pixels.length; i += 4, pixelIndex++) {
                    const r = pixels[i];
                    const g = pixels[i + 1];
                    const b = pixels[i + 2];
                    const gray = Math.round(0.299 * r + 0.587 * g + 0.114 * b);
                    grayScaleBytes[pixelIndex] = gray;
                    pixels[i] = gray;
                    pixels[i + 1] = gray;
                    pixels[i + 2] = gray;
                }
    
                ctx.putImageData(imageData, 0, 0);
                resolve({
                    previewUrl: canvas.toDataURL('image/png'),
                    bytes: grayScaleBytes,
                    width: canvas.width,
                    height: canvas.height
                });
            };

            img.onerror = reject;
            img.src = URL.createObjectURL(file);
        });
	}
}
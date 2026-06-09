import { Component, inject, signal, computed } from '@angular/core';
import { Router } from '@angular/router';
import { PageHeaderComponent } from '../../../shared/components/page-header/page-header.component';
import { CardComponent } from '../../../shared/components/card/card.component';
import { ButtonComponent } from '../../../shared/components/button/button.component';
import { ImageFileService } from '../services/image-file.service';
import { TfheService } from '../../../core/crypto/tfhe.service';
import { ImageServerService } from '../services/image-server.service';
import { AlertComponent } from '../../../shared/components/alert/alert.component';
import { LoadingOverlayComponent } from '../../../shared/components/loading-overlay/loading-overlay.component';
import { ImageProcessingService } from '../services/image-processing.service';
import { DecryptedData, EncryptedData, ImageEncryptionService } from '../services/image-encryption.service';
import { ImageUrlService } from '../services/image-url.service';
import { Observable } from 'rxjs';
import { KeyStoreService } from '../services/key-store.service';

@Component({
  	selector: 'app-image-editor',
  	standalone: true,
  	imports: [
		PageHeaderComponent, 
		CardComponent, 
		ButtonComponent,
		AlertComponent, 
		LoadingOverlayComponent],
  	templateUrl: './image-editor.component.html',
  	styleUrl: './image-editor.component.css',
})
export class ImageEditorComponent {
	public imageService = inject(ImageFileService);
	public processingService = inject(ImageProcessingService);
	private encryptionService = inject(ImageEncryptionService);
	private tfhe = inject(TfheService);
	private imageServer = inject(ImageServerService);
	private urlService = inject(ImageUrlService);
	private router = inject(Router);
	private keyStore = inject(KeyStoreService);

	image = this.processingService.processedImage();
	imageName = this.imageService.imageName();
	editedImage = signal<string | null>(null);

	isLoading = signal(false);
	hasKeys = signal(false);
	hasSession = signal(false);

	errorMessage = signal<string | null>(null);
	successMessage = signal<string | null>(null);
	loadingMessage = signal<string | null>(null);

	ngOnInit(): void {
		this.getSessionStatus();
		const keyPair = this.keyStore.keyPair();
		if (!keyPair) {
			this.hasKeys.set(false);
			return;
		}
		this.hasKeys.set(true);
	}

	async generateKeys(): Promise<void> {
		this.successMessage.set(null);
		this.errorMessage.set(null);
		this.isLoading.set(true);
		this.loadingMessage.set('Schlüssel werden erzeugt...');

		await new Promise((r) => setTimeout(r, 50));
		try {
			await this.tfhe.ensureInitialized();
			const kp = this.tfhe.generateKeyPair();
			this.keyStore.addKey({
				...kp
			});
			this.hasKeys.set(true);
			this.successMessage.set('Schlüssel erfolgreich erzeugt.');
		} catch (e) {
			console.error('Key generation failed', e);
			this.errorMessage.set('Schlüsselgenerierung fehlgeschlagen: ' + (e as Error).message);
		} finally {
			this.isLoading.set(false);
		}
	}

	getSessionStatus(): void {
		this.imageServer.getStatus().subscribe({
			next: (res) => {
				this.hasSession.set(res.session_active)
			},
			error: (err) => {
				console.error('Load Session Status failed.', err);
			}
		});
	}

	async createSession(): Promise<void> {
		if (!this.keyStore.keyPair()) {
			this.errorMessage.set('Keine Keys vorhanden, bitte erstelle zuerst neue Keys.');
			return;
		}
		this.errorMessage.set(null);
		this.successMessage.set(null);
		this.isLoading.set(true);
		this.loadingMessage.set('Session wird erstellt...');
	
		const imageData = this.processingService.processedImage();
		if (!imageData) return;
		const decryptedImage: DecryptedData = {
			bytes: imageData.bytes,
			width: imageData.width,
			height: imageData.height
		}

		const keyPair = this.keyStore.keyPair();
		if (!keyPair) return;
		const encryptedImage = await this.encryptionService.encryptImage(decryptedImage, keyPair.clientKey);
		const compressed_server_key = Array.from(keyPair.serverKeyBytes);
		const image_data = encryptedImage.bytes.map(pixel => Array.from(pixel))

		this.imageServer
		.createSession({
			compressed_server_key: compressed_server_key,
			image_data: image_data,
			width: encryptedImage.width,
			height: encryptedImage.height
		})
		.subscribe({
        next: (res) => {
			this.successMessage.set('Session erfolgreich erstellt.');
			this.isLoading.set(false);
			this.hasSession.set(true);
		},
		error: (err) => {
			console.error('Create session failed', err);
          	this.errorMessage.set(
            	'Session konnte nicht erstellt werden. Bitte später erneut versuchen.',
          	);
          	this.isLoading.set(false);
		} })
	}

	async finalizeSession(): Promise<void> {
		if (!this.keyStore.keyPair()) {
			this.errorMessage.set('Keine Keys vorhanden.');
			return;
		}
		this.errorMessage.set(null);
		this.successMessage.set(null);
		this.isLoading.set(true);
		this.loadingMessage.set('Session wird finalisiert...');

		this.imageServer.finalizeSession().subscribe({
		next: async (res) => {
			const width = res.width;
			const height = res.height;
			const encryptedPixels: Uint8Array[] = res.image_data.map(
				pixel => new Uint8Array(pixel)
			);

			const encryptedImage: EncryptedData = {
				bytes: encryptedPixels,
				width: width,
				height: height
			}

			const keyPair = this.keyStore.keyPair();
			if (!keyPair) return;
			const decryptedImage = await this.encryptionService.decryptImage(
				encryptedImage, 
				keyPair.clientKey
			);

			const previewUrl = this.urlService.createPreviewUrl(
				decryptedImage.bytes,
				decryptedImage.width,
				decryptedImage.height
			)

			this.editedImage.set(previewUrl);

			this.successMessage.set('Session erfolgreich finalisiert.');
			this.isLoading.set(false);
			this.hasSession.set(false);
		},
		error: (err) => {
			console.error('Finalize session failed', err);
          	this.errorMessage.set(
            	'Session konnte nicht finalisiert werden. Bitte später erneut versuchen.',
          	);
          	this.isLoading.set(false);
		} })
	}

	exitEditor(): void {
		this.router.navigateByUrl('/image-processing');
	}

	imageOperation(
		request: Observable<any>,
		keyword: string 
	): void {
		this.errorMessage.set(null);
		this.successMessage.set(null);
		this.isLoading.set(true);
		this.loadingMessage.set(keyword + ' wird ausgeführt...');

		request.subscribe({
			next: (res) => {
				this.successMessage.set(keyword + ' wurde erfolgreich durchgeführt.');
				this.isLoading.set(false);
			},
			error: (err) => {
				console.error('Finalize session failed', err);
          		this.errorMessage.set(
            		keyword + ' konnte nicht durchgeführt werden. Bitte später erneut versuchen.',
          		);
          		this.isLoading.set(false);
			},
		})
	}

	invert(): void {
		this.imageOperation(
			this.imageServer.invert(),
			'Invertieren'
		);
	}

	whiteThreshhold(): void {
		this.imageOperation(
			this.imageServer.white_threshhold(),
			'Weiß-Grenzwert'
		);
	}

	blackThreshhold(): void {
		this.imageOperation(
			this.imageServer.black_threshhold(),
			'Schwarz-Grenzwert'
		);
	}

	rotate90(): void {
		this.imageOperation(
			this.imageServer.rotate_90(),
			'Drehen um 90°'
		);
	}

	rotate180(): void {
		this.imageOperation(
			this.imageServer.rotate_180(),
			'Drehen um 180°'
		);
	}

	rotate270(): void {
		this.imageOperation(
			this.imageServer.rotate_270(),
			'Drehen um 270°'
		);
	}

	flipVertical(): void {
		this.imageOperation(
			this.imageServer.flip_vertical(),
			'Vertikales Spiegeln'
		);
	}

	flipHorizontal(): void {
		this.imageOperation(
			this.imageServer.flip_horizontal(),
			'Horizontales Spiegeln'
		);
	}

	bloom(): void {
		this.imageOperation(
			this.imageServer.bloom(),
			'Blooming'
		);
	}

	blur(): void {
		this.imageOperation(
			this.imageServer.blur(),
			'Blurring'
		);
	}
}
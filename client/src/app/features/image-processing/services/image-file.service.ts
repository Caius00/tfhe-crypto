import { Injectable, signal } from "@angular/core";

@Injectable({
    providedIn: 'root'
})
export class ImageFileService {
    imageFile = signal<File | null>(null);
    imageName = signal<string>('');

    setImage(file: File) {
        this.imageFile.set(file);
        this.imageName.set(file.name);
    }
}
import { Component, inject } from "@angular/core";
import { ButtonComponent } from "../../../../shared/components/button/button.component";
import { CommonModule } from "@angular/common";
import { ImageFileService } from "../../services/image-file.service";

@Component({
    selector: 'app-file-upload',
    imports: [ButtonComponent, CommonModule],
    templateUrl: 'file-upload.component.html',
    styleUrl: 'file-upload.component.css'
})
export class FileUploadComponent {
    public imageService = inject(ImageFileService);

    async onFileSelected(event: Event) {
        const input = event.target as HTMLInputElement;

        if (!input.files?.length) {
            return 
        }

        const file = input.files[0];

        if (file.type !== 'image/png') {
            alert('Die hochgeladene Datei ist nicht im PNG-Format!')
            input.value = '';
            return;
        }

        this.imageService.setImage(file);
    }
}
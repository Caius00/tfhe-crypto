import { Routes } from '@angular/router';
import { ImageProcessingComponent } from './image-processing.component';
import { ImageEditorComponent } from './editor/image-editor.component';

export const IMAGE_PROCESSING_ROUTES: Routes = [
  { path: '', component: ImageProcessingComponent },
  { path: 'editor', component: ImageEditorComponent }
];

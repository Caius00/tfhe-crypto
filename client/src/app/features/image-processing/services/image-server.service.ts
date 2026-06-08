import { Injectable, inject } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { Observable } from 'rxjs';
import { SERVICE_URLS } from '../../../core/api/service-urls';

export interface ApiResponse {
    success: boolean;
    message: string;
}

export interface CreateSessionRequest {
    compressed_server_key: number[];
    image_data: number[][];
    width: number;
    height: number;
}

export interface DeleteSessionResponse {
    image_data: number[][];
    width: number;
    height: number;
}

export interface StatusResponse {
    session_active: boolean
}

@Injectable({ providedIn: 'root' })
export class ImageServerService {
    private http = inject(HttpClient);
    private baseUrl = SERVICE_URLS.imageProcessing.path;

    getStatus(): Observable<StatusResponse> {
        return this.http.get<StatusResponse>(
            `${this.baseUrl}/status`, { }
        )
    }

    createSession(
        createSessionRequest: CreateSessionRequest
    ) {
        return this.http.post(
            `${this.baseUrl}/session`, { 
                compressed_server_key: createSessionRequest.compressed_server_key, 
                image_data: createSessionRequest.image_data,
                width: createSessionRequest.width,
                height: createSessionRequest.height
            }
        );
    }

    finalizeSession(
    ) : Observable<DeleteSessionResponse> {
        return this.http.delete<DeleteSessionResponse>(
            `${this.baseUrl}/session`, { }
        );
    }

    invert() {
        return this.http.post(
            `${this.baseUrl}/per-pixel/invert`, { }
        )
    }

    white_threshhold() {
        return this.http.post(
            `${this.baseUrl}/per-pixel/white-threshhold`, { }
        )
    }

    black_threshhold() {
        return this.http.post(
            `${this.baseUrl}/per-pixel/black_threshhold`, { }
        )
    }

    rotate_90() {
        return this.http.post(
            `${this.baseUrl}/rotate/90`, { }
        )
    }

    rotate_180() {
        return this.http.post(
            `${this.baseUrl}/rotate/180`, { }
        )
    }

    rotate_270() {
        return this.http.post(
            `${this.baseUrl}/rotate/270`, { }
        )
    }

    flip_vertical() {
        return this.http.post(
            `${this.baseUrl}/flip/vertical`, { }
        )
    }

    flip_horizontal() {
        return this.http.post(
            `${this.baseUrl}/flip/horizontal`, { }
        )
    }

    blur() {
        return this.http.post(
            `${this.baseUrl}/effects/blur`, { }
        )
    }

    bloom() {
        return this.http.post(
            `${this.baseUrl}/effects/bloom`, { }
        )
    }
}
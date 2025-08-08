export interface OutputStream {
    write(bytes: Uint8Array): void;
}

export interface ModuleOptions {
    default_fds?: {
        [key: number]: OutputStream;
    };
    initial_log_filter?: string;
}

export class Subtitles {
    private constructor();
    static parseFromString(text: string | Uint8Array): Subtitles;
}

export class Framebuffer {
    constructor(width: number, height: number);
    resize(width: number, height: number): void;
    imageData(): ImageData;
    imageBitmap(): Promise<ImageBitmap>;
    free(): void;
}

export interface SubtitleContext {
    dpi?: number;
    video_width: number;
    video_height: number;
    padding_left?: number;
    padding_right?: number;
    padding_top?: number;
    padding_bottom?: number;
}

export class Renderer {
    constructor();
    // TODO: rework for new font handling system somehow
    addFont(name: string, weight: number | [number, number] | "auto", italic: boolean, data: Uint8Array): void;
    setSubtitles(subtitles: Subtitles): void;
    render(ctx: SubtitleContext, fb: Framebuffer, t: number): void;
    destroy(): void;
}

export function initStreaming(source: Response | PromiseLike<Response>, options?: ModuleOptions): Promise<void>;

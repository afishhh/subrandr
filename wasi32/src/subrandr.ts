import { LibraryPtr, RendererPtr, SubrandrModule, ModuleOptions, SubtitlesPtr, FontPtr } from "./module.js";
import { structField, WASM_NULL, WasmPtr, writeStruct } from "./wasm_utils.js";

export class Subtitles {
  /** @internal */
  __ptr: SubtitlesPtr;

  private constructor(ptr: SubtitlesPtr) {
    this.__ptr = ptr
  }

  public static parseFromString(text: string | Uint8Array): Subtitles {
    const g = state();
    const [text_ptr, text_len] = g.mod.allocCopy(text)
    let ptr: SubtitlesPtr;
    try {
      ptr = g.mod.exports.sbr_load_text(
        g.lib,
        text_ptr,
        text_len,
        /* SBR_SUBTITLE_FORMAT_UNKNOWN */ 0,
        WASM_NULL
      )
    } finally {
      g.mod.dealloc(text_ptr, text_len)
    }

    return new Subtitles(ptr)
  }

  destroy() {
    const g = state();
    g.mod.exports.sbr_subtitles_destroy(this.__ptr)
  }
}

// TODO: this memory should be freed on GC too
export class Framebuffer {
  /** @internal */
  _front: PixelBuffer;
  /** @internal */
  _back: PixelBuffer;
  /** @internal */
  _width: number
  /** @internal */
  _height: number

  constructor(width: number, height: number) {
    const g = state();
    const npixels = this._checkSize(width, height)
    const front = new PixelBuffer(g.mod, npixels)
    const back = new PixelBuffer(g.mod, npixels)
    this._front = front
    this._back = back
    this._width = width
    this._height = height
  }

  private _checkSize(width: number, height: number) {
    if(!Number.isSafeInteger(width) || !Number.isSafeInteger(height))
      throw Error("Framebuffer width or height is not a safe integer")

    const pixels = width * height
    if(!Number.isSafeInteger(4 * pixels))
      throw Error("Framebuffer size overflow")

    return pixels
  }

  public resize(width: number, height: number) {
    const g = state();
    const newpixels = this._checkSize(width, height)

    // TODO: is this reasonable reallocation behaviour?
    //       add some logs and check
    const diff = this._front.npixels - newpixels
    if (diff > newpixels * 2 / 3) {
      this._front.realloc(g.mod, newpixels)
      this._back.realloc(g.mod, newpixels)
    } else if (diff < 0) {
      this._front.realloc(g.mod, newpixels * 2)
      this._back.realloc(g.mod, newpixels * 2)
    }

    this._width = width
    this._height = height
  }

  public imageData(): ImageData {
    const g = state();
    const frontPixels = new Uint8ClampedArray(g.mod.memoryBuffer, this._front.ptr, this._width * this._height * 4)
    const imageData = new ImageData(frontPixels, this._width, this._height, {
      colorSpace: "srgb"
    })
    return imageData
  }

  public async imageBitmap() {
    const data = this.imageData()
    return await globalThis.createImageBitmap(
      data,
      0,
      0,
      data.width,
      data.height,
      { premultiplyAlpha: "premultiply" }
    )
  }

  free() {
    const g = state();
    this._front.free(g.mod)
    this._back.free(g.mod)
  }
}

export class Font {
  /** @internal */
  __ptr: FontPtr;

  constructor(data: Uint8Array) {
    const g = state();
    const font_data_ptr = g.mod.exports.sbr_wasm_create_uninit_arc(data.length);
    g.mod.memoryBytes.set(data, font_data_ptr);
    try {
      const font = g.mod.exports.sbr_wasm_library_create_font(
        g.lib,
        font_data_ptr,
        data.length
      )
      if(font == WASM_NULL)
        g.mod.handleError()
      this.__ptr = font
    } finally {
      g.mod.exports.sbr_wasm_destroy_arc(font_data_ptr, data.length)
    }
  }

  close() {
    const g = state();
    g.mod.exports.sbr_library_close_font(g.lib, this.__ptr)
    this.__ptr = WASM_NULL as FontPtr
  }
}

const SUBCTX_LEN = 28

export interface SubtitleContext {
  dpi?: number,
  video_width: number,
  video_height: number,
  padding_left?: number,
  padding_right?: number,
  padding_top?: number,
  padding_bottom?: number,
}

export class Renderer {
  /** @internal */
  __ptr: RendererPtr;
  /** @internal */
  __ctxptr: WasmPtr;

  constructor() {
    const g = state();
    this.__ptr = g.mod.exports.sbr_renderer_create(g.lib);
    this.__ctxptr = g.mod.alloc(SUBCTX_LEN)
  }

  addFont(name: string, weight: number | [number, number] | "auto", italic: boolean, face: Font) {
    let weights: [number, number];
    if (typeof weight == "number") {
      weights = [weight, weight]
    } else if (weight == "auto") {
      weights = [-1, -1]
    } else {
      weights = weight
    }

    if(weight != "auto")
      for (const w of weights)
        if (!Number.isSafeInteger(w) || w < 0 || w > 1000)
          throw new RangeError("font weight must be an integer in the range 0..=1000")

    const g = state();
    const [name_ptr, name_len] = g.mod.allocCopy(name)
    try {
      g.mod.exports.sbr_wasm_renderer_add_font(
        this.__ptr,
        name_ptr,
        name_len,
        weights[0],
        weights[1],
        italic,
        face.__ptr
      )
    } finally {
      g.mod.dealloc(name_ptr, name_len)
    }
  }

  render(ctx: SubtitleContext, fb: Framebuffer, subs: Subtitles, t: number) {
    const g = state();
    writeStruct(
      new DataView(g.mod.memoryBuffer, this.__ctxptr, SUBCTX_LEN),
      [
        structField("u32", ctx.dpi ?? 72 * window.devicePixelRatio),
        structField("i32", ctx.video_width << 6),
        structField("i32", ctx.video_height << 6),
        structField("i32", (ctx.padding_left ?? 0) << 6),
        structField("i32", (ctx.padding_right ?? 0)),
        structField("i32", (ctx.padding_top ?? 0) << 6),
        structField("i32", (ctx.padding_bottom ?? 0) << 6),
      ]);

    const front = fb._front;
    const back = fb._back;
    g.mod.exports.sbr_renderer_render(
      this.__ptr,
      this.__ctxptr,
      subs.__ptr,
      t,
      back.ptr,
      fb._width,
      fb._height,
      fb._width,
    );
    g.mod.exports.sbr_wasm_copy_convert_to_rgba(
      front.ptr,
      back.ptr,
      fb._width,
      fb._height
    )
  }

  destroy() {
    const g = state();
    g.mod.exports.sbr_wasm_dealloc(this.__ctxptr, SUBCTX_LEN)
    g.mod.exports.sbr_renderer_destroy(this.__ptr)
    this.__ptr = WASM_NULL as RendererPtr
    this.__ctxptr = WASM_NULL
  }
}

interface State {
  mod: SubrandrModule,
  lib: LibraryPtr
}

let STATE: State | null = null
function state() {
  const mod = STATE;
  if (mod === null) throw Error("subrandr library not initialized yet, call initStreaming() first!")
  return mod
}

export async function initStreaming(source: Response | PromiseLike<Response>, options?: ModuleOptions) {
  const mod = await SubrandrModule.instantiateStreaming(source, options)
  const lib = mod.exports.sbr_library_init()
  STATE = {
    mod,
    lib
  }
}

class PixelBuffer {
  ptr;
  npixels;

  constructor(mod: SubrandrModule, npixels: number) {
    this.ptr = mod.alloc(4 * npixels);
    this.npixels = npixels;
  }

  realloc(mod: SubrandrModule, npixels: number) {
    this.free(mod)
    this.ptr = mod.alloc(4 * npixels)
    this.npixels = npixels
  }

  free(mod: SubrandrModule) {
    mod.dealloc(this.ptr, this.byteLength)
    this.ptr = WASM_NULL
  }

  get byteLength() {
    return 4 * this.npixels
  }
}

import { LibraryPtr, RendererPtr, SubrandrModule, ModuleOptions, SubtitlesPtr } from "./module.js";
import { structField, WASM_NULL, WasmPtr, writeStruct } from "./wasm_utils.js";

export class Subtitles {
  /** @internal */
  __ptr: SubtitlesPtr;

  private constructor(ptr: SubtitlesPtr) {
    this.__ptr = ptr
  }

  // TODO: use filename
  public static parseFromString(text: string | Uint8Array, _filename?: string): Subtitles {

    const g = state();
    const [text_ptr, text_len] = g.mod.allocCopy(text)
    let ptr: SubtitlesPtr;
    try {
      ptr = g.mod.exports.sbr_wasm_load_subtitles(g.lib, text_ptr, text_len)
    } finally {
      g.mod.dealloc(text_ptr, text_len)
    }

    return new Subtitles(ptr)
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
    const front = new PixelBuffer(g.mod, width * height)
    const back = new PixelBuffer(g.mod, width * height)
    this._front = front
    this._back = back
    this._width = width
    this._height = height
  }

  public resize(width: number, height: number) {
    const g = state();
    const newpixels = width * height

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
    const frontPixels = new Uint8ClampedArray(g.mod.memoryBuffer, this._front.ptr, this._front.byteLength)
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

  constructor(subtitles: Subtitles) {
    const g = state();
    this.__ptr = g.mod.exports.sbr_renderer_create(
      g.lib,
      subtitles.__ptr
    );
    this.__ctxptr = g.mod.alloc(SUBCTX_LEN)
  }

  addFont(name: string, weight: number | "variadic", italic: boolean, data: Uint8Array) {
    if (weight != "variadic" && !(weight > 0 && Number.isInteger(weight)))
      throw new RangeError("font weight must be a positive integer")

    const g = state();
    const font_ptr = g.mod.exports.sbr_wasm_create_uninit_arc(data.length);
    g.mod.memoryBytes.set(data, font_ptr);

    const [name_ptr, name_len] = g.mod.allocCopy(name)
    try {
      g.mod.exports.sbr_wasm_renderer_add_font(
        this.__ptr,
        name_ptr,
        name_len,
        weight == "variadic" ? -1 : weight,
        italic,
        font_ptr,
        data.length
      )
    } finally {
      g.mod.dealloc(name_ptr, name_len)
    }
  }

  render(ctx: SubtitleContext, fb: Framebuffer, t: number) {
    const g = state();
    writeStruct(
      new DataView(g.mod.memoryBuffer, this.__ctxptr, SUBCTX_LEN),
      [
        structField("u32", ctx.dpi ?? 72 * window.devicePixelRatio),
        structField("f32", ctx.video_width),
        structField("f32", ctx.video_height),
        structField("f32", ctx.padding_left ?? 0),
        structField("f32", ctx.padding_right ?? 0),
        structField("f32", ctx.padding_top ?? 0),
        structField("f32", ctx.padding_bottom ?? 0),
      ]);

    const front = (fb as any)._front as PixelBuffer;
    const back = (fb as any)._back as PixelBuffer;
    g.mod.exports.sbr_renderer_render(
      this.__ptr,
      this.__ctxptr,
      t,
      back.ptr,
      fb._width,
      fb._height,
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
  // "/target/wasm32-wasip1/debug/subrandr.wasm"
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

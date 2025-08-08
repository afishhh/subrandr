import { CStructData, decodeAndEscapeControl, sizeStruct, UTF8_DECODER, UTF8_ENCODER, WASM_NULL, WasmPtr, writeStruct } from "./wasm_utils.js";

if ("window" in globalThis && !window.crossOriginIsolated)
	console.warn("subrandr: not cross origin isolated, clock precision will suffer")

export type LibraryPtr = WasmPtr & { readonly __lib_tag: unique symbol };
export type SubtitlesPtr = WasmPtr & { readonly __sub_tag: unique symbol };
export type RendererPtr = WasmPtr & { readonly __renderer_tag: unique symbol };
export type FontPtr = WasmPtr & { readonly __font_tag: unique symbol };

class SubrandrError extends Error {
	/** @internal */
	constructor(message: string) {
		super(message)
	}
}

export interface SubrandrExports {
	memory: WebAssembly.Memory

	// Wasm utility API
	sbr_wasm_alloc(size: number): WasmPtr
	sbr_wasm_dealloc(ptr: WasmPtr, size: number): void
	sbr_wasm_create_uninit_arc(size: number): WasmPtr
	sbr_wasm_destroy_arc(ptr: WasmPtr, size: number): void

	// C API
	sbr_library_init(): LibraryPtr
	sbr_library_fini(ptr: LibraryPtr): void
	sbr_library_close_font(sbr: LibraryPtr, ptr: FontPtr): void
	sbr_load_text(
		ptr: LibraryPtr,
		text_ptr: WasmPtr, text_len: number,
		format: number, language_hint: WasmPtr
	): SubtitlesPtr;
	sbr_renderer_create(sbr: LibraryPtr): RendererPtr
	sbr_renderer_set_subtitles(
		renderer: RendererPtr,
		subs: SubtitlesPtr,
	): void;
	sbr_renderer_render(
		renderer: RendererPtr,
		subtitle_context: WasmPtr,
		t: number,
		buf_ptr: WasmPtr,
		buf_width: number,
		buf_height: number,
		buf_stride: number
	): void
	sbr_renderer_destroy(renderer: RendererPtr): void
	sbr_subtitles_destroy(subtitles: SubtitlesPtr): void
	sbr_get_last_error_string(): WasmPtr

	// Wasm specific API
	sbr_wasm_library_create_font(sbr: LibraryPtr, arc_data_ptr: WasmPtr, arc_data_len: number): FontPtr
	sbr_wasm_renderer_add_font(
		renderer: RendererPtr,
		name_ptr: WasmPtr,
		name_len: number,
		weight0: number,
		weight1: number,
		italic: boolean,
		font_ptr: FontPtr,
	): void
	sbr_wasm_copy_convert_to_rgba(
		front: WasmPtr, back: WasmPtr,
		width: number, height: number
	): void
}

export interface OutputStream {
	write(bytes: Uint8Array): void;
}

export class ConsoleOutputStream implements OutputStream {
	private _name: string;
	private _buffer = new Uint8Array;

	public constructor(name: string) {
		this._name = name;
	}

	// nice
	private _append(data: Uint8Array) {
		const old = this._buffer;
		this._buffer = new Uint8Array(old.length + data.length);
		this._buffer.set(old, 0);
		this._buffer.set(data, old.length);
	}

	private _log() {
		console.log("subrandr.wasm/" + this._name + ":", decodeAndEscapeControl(this._buffer))
		this._buffer = new Uint8Array
	}

	write(bytes: Uint8Array) {
		while (bytes.length > 0) {
			const newline = bytes.findIndex(b => b == 0x0A);
			const end = newline == -1 ? bytes.length : newline + 1;
			const appendEnd = newline == -1 ? bytes.length : newline;

			this._append(bytes.subarray(0, appendEnd))
			if (newline != -1) {
				this._log()
				bytes = bytes.subarray(end);
			} else
				break;
		}
	}
}

function prepareVariables(vars: { [key: string]: string }) {
	const result = [];
	let size = 0;
	for (const name in vars) {
		const key = UTF8_ENCODER.encode(name)
		const value = UTF8_ENCODER.encode(vars[name])
		const keyvalue = new Uint8Array(key.length + value.length + 2);
		keyvalue.set(key, 0);
		keyvalue[key.length] = 0x3D; // '='
		keyvalue.set(value, key.length + 1);
		result.push(keyvalue);
		size += keyvalue.length;
	}
	return { items: result, total_size: size };
}

export interface ModuleOptions {
	default_fds?: { [key: number]: OutputStream },
	initial_log_filter?: string
}

export class SubrandrModule {
	instance: WebAssembly.Instance

	get exports() { return this.instance.exports as unknown as SubrandrExports }
	get memoryBuffer() { return this.exports.memory.buffer }
	get memoryBytes() { return new Uint8Array(this.exports.memory.buffer) }
	get memoryView() { return new DataView(this.memoryBuffer) }

	private constructor(instance: WebAssembly.Instance) {
		this.instance = instance;
	}

	public static async instantiateStreaming(source: Response | PromiseLike<Response>, options?: ModuleOptions) {
		const output_streams = options?.default_fds ?? {
			1: new ConsoleOutputStream("stdout"),
			2: new ConsoleOutputStream("stderr"),
		};

		const VARIABLES = prepareVariables({
			"SBR_LOG": options?.initial_log_filter ?? "info"
		})

		let self: SubrandrModule;
		const instantiated = await WebAssembly.instantiateStreaming(source, {
			env: {},
			wasi_snapshot_preview1: {
				environ_get: (environ: number, environ_buf: number) => {
					const array = self.memoryBytes;
					const view = self.memoryView;
					let current = environ_buf;
					for (let i = 0; i < VARIABLES.items.length; ++i) {
						const item = VARIABLES.items[i];
						array.set(item, current);
						view.setUint32(environ + i * 4, current, true);
						current += item.length;
					}
					return 0;
				},
				environ_sizes_get: (environ_count: number, environ_buf_size: number) => {
					const view = self.memoryView;
					view.setUint32(environ_count, VARIABLES.items.length, true);
					view.setUint32(environ_buf_size, VARIABLES.total_size, true);
					return 0;
				},
				fd_close() {
					throw Error("fd_close");
				},
				fd_fdstat_get(fd: number, buf_ptr: number) {
					const view = self.memoryView;
					if (fd in output_streams) {
						// filetype = regular
						view.setUint8(buf_ptr, 4);
						// flags = append
						view.setUint16(buf_ptr + 2, 1, true);
						// rights = write
						view.setUint32(buf_ptr + 8, 1 << 5, true);
						view.setUint32(buf_ptr + 12, 0, true);
						return 0;
					}

					throw Error("fdstat_get");
				},
				fd_fdstat_set_flags() {
					throw Error("fdstat_set_flags");
				},
				fd_prestat_get() {
					throw Error("fdstat_prestat_get");
				},
				fd_prestat_dir_name() {
					throw Error("fdstat_prestat_dir_name");
				},
				fd_read() {
					throw Error("read");
				},
				fd_seek() {
					throw Error("seek");
				},
				fd_write(fd: number, iovs_ptr: number, iovs_len: number, ret_ptr: number) {
					const view = self.memoryView;
					// console.log(`fd_write(${fd}, ${iovs}, ${iovs_len}, ${ret_ptr})`)
					if (!(fd in output_streams))
						throw Error(`write(${fd}, ...)`);
					const stream = output_streams[fd];

					let total = 0;
					for (let i = 0; i < iovs_len; ++i) {
						const iov_off = (iovs_ptr + i * 8);
						const buf = view.getUint32(iov_off, true);
						const len = view.getUint32(iov_off + 4, true);

						stream.write(new Uint8Array(view.buffer, buf, len));

						total += len;
					}
					view.setUint32(ret_ptr, total, true);

					return 0;
				},
				fd_filestat_get() {
					throw Error("fd_filestat_get");
				},
				random_get(ptr: number, len: number) {
					globalThis.crypto.getRandomValues(new Uint8Array(self.memoryBuffer, ptr, len));
					return 0;
				},
				path_open() {
					throw Error("open");
				},
				proc_exit() {
					throw Error("exit");
				},
				clock_time_get(clockid: number, _precision: number, ret: number) {
					if (clockid != 1)
						throw Error("clock_time_get non-monotonic");
					const millis = globalThis.performance.now();
					const upper = BigInt(Math.trunc(millis));
					const lower = BigInt(Math.trunc((millis % 1) * 1000000));
					const nanos = upper * BigInt(1000000) + lower;
					self.memoryView.setBigUint64(ret, nanos, true);
					return 0;
				}
			}
		});
		self = new SubrandrModule(instantiated.instance);
		return self;
	}

	alloc(len: number): WasmPtr {
		return this.exports.sbr_wasm_alloc(len)
	}

	allocCopy(value: string | Uint8Array): [WasmPtr, number] {
		let bytes: Uint8Array;
		if (typeof value == "string")
			bytes = UTF8_ENCODER.encode(value)
		else
			bytes = value;

		const ptr = this.alloc(bytes.length)
		this.memoryBytes.set(bytes, ptr)
		return [ptr, bytes.length]
	}

	allocStruct(data: CStructData): [WasmPtr, number] {
		const len = sizeStruct(data)
		const ptr = this.alloc(len)
		writeStruct(new DataView(this.memoryBuffer, ptr, len), data)
		return [ptr, len]
	}

	dealloc(ptr: WasmPtr, len: number) {
		this.exports.sbr_wasm_dealloc(ptr, len)
	}

	readCString(ptr: WasmPtr): Uint8Array {
		const memoryBytes = this.memoryBytes
		let end = ptr as number
		while(memoryBytes[end] != 0)
			++end;
		return memoryBytes.subarray(ptr, end)
	}

	handleError(): never {
		const errorPtr = this.exports.sbr_get_last_error_string()
		if(errorPtr != WASM_NULL) {
			const message = this.readCString(errorPtr)
			throw new SubrandrError(UTF8_DECODER.decode(message))
		}
		throw new SubrandrError("unknown error: handleError called but get_last_error_string returned null")
	}
}

export type WasmPtr = number & { readonly __ptr_tag: unique symbol };
export const WASM_NULL = 0 as WasmPtr

export const UTF8_DECODER = new TextDecoder();
export const UTF8_ENCODER = new TextEncoder();

export function decodeAndEscapeControl(buf: Uint8Array) {
	const decoded = UTF8_DECODER.decode(buf);
	let result = ""
	for (let i = 0; i < decoded.length; ++i) {
		let chr = decoded[i];
		if (chr == "\n")
			chr = "\\n";
		else if (chr == "\x1b")
			chr = "\\e"
		result += chr;
	}
	return result
}


type CFieldType = "f32" | "i32" | "u32";
interface CFieldValues {
	f32: number,
	i32: number,
	u32: number,
}

const C_TYPE_ALIGNMENT = {
	"f32": 4,
	"i32": 4,
	"u32": 4,
};

const C_TYPE_SIZE = C_TYPE_ALIGNMENT;

type CStructField<T extends CFieldType> = { type: T, value: CFieldValues[T] }
export function structField<T extends CFieldType>(type: T, value: CFieldValues[T]): CStructField<T> {
	return {
		type,
		value
	}
}

export type CStructData = CStructField<CFieldType>[];

export function sizeStruct(fields: CStructData): number {
	let offset = 0
	for (const field of fields) {
		const alignment = C_TYPE_ALIGNMENT[field.type];
		if (offset % alignment != 0)
			offset += alignment - (offset % alignment);
		offset += C_TYPE_SIZE[field.type];
	}
	return offset
}

export function writeStruct(output: DataView, fields: CStructData) {
	let offset = 0
	for (const field of fields) {
		const alignment = C_TYPE_ALIGNMENT[field.type];
		if (offset % alignment != 0)
			offset += alignment - (offset % alignment);

		switch (field.type) {
			case "f32":
				output.setFloat32(offset, field.value, true)
				break;
			case "i32":
				output.setInt32(offset, field.value, true)
				break;
			case "u32":
				output.setUint32(offset, field.value, true)
				break;
		}

		offset += C_TYPE_SIZE[field.type];
	}
}

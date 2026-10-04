/** Same plain linear-memory ABI as the PDF and XLSX engines. */
export interface Exports {
  memory: WebAssembly.Memory;
  imprenta_alloc(len: number): number;
  imprenta_dealloc(ptr: number, len: number): void;
  imprenta_assets_reset(): number;
  imprenta_assets_image(namePtr: number, nameLen: number, dataPtr: number, dataLen: number): number;
  imprenta_render(ptr: number, len: number): number;
  imprenta_out_ptr(): number;
  imprenta_out_len(): number;
  imprenta_meta_ptr(): number;
  imprenta_meta_len(): number;
  imprenta_out_release(): number;
  imprenta_error_ptr(): number;
  imprenta_error_len(): number;
}
const REQUIRED = [
  'imprenta_alloc',
  'imprenta_dealloc',
  'imprenta_assets_reset',
  'imprenta_assets_image',
  'imprenta_render',
  'imprenta_out_ptr',
  'imprenta_out_len',
  'imprenta_meta_ptr',
  'imprenta_meta_len',
  'imprenta_out_release',
  'imprenta_error_ptr',
  'imprenta_error_len',
] as const;
export type WasmSource = BufferSource | WebAssembly.Module;
export async function compile(source: WasmSource): Promise<WebAssembly.Module> {
  return source instanceof WebAssembly.Module ? source : WebAssembly.compile(source);
}
export async function instantiate(module: WebAssembly.Module): Promise<Exports> {
  const { exports } = await WebAssembly.instantiate(module, {});
  if (!(exports.memory instanceof WebAssembly.Memory))
    throw new Error('the receipt module is missing memory');
  const wrapped: Record<string, unknown> = { memory: exports.memory };
  for (const name of REQUIRED) {
    const value = exports[name];
    if (typeof value !== 'function') throw new Error(`the receipt module is missing ${name}`);
    wrapped[name] = (...args: number[]) => (value as (...args: number[]) => number)(...args) >>> 0;
  }
  return wrapped as unknown as Exports;
}
export class Memory {
  constructor(private readonly e: Exports) {}
  write(data: Uint8Array): [number, number] {
    if (!data.length) return [0, 0];
    const ptr = this.e.imprenta_alloc(data.length);
    new Uint8Array(this.e.memory.buffer).set(data, ptr);
    return [ptr, data.length];
  }
  writeText(text: string): [number, number] {
    return this.write(encoder.encode(text));
  }
  free([ptr, len]: [number, number]): void {
    if (len) this.e.imprenta_dealloc(ptr, len);
  }
  read(ptr: number, len: number): Uint8Array {
    return new Uint8Array(this.e.memory.buffer, ptr, len).slice();
  }
  readText(ptr: number, len: number): string {
    return decoder.decode(new Uint8Array(this.e.memory.buffer, ptr, len));
  }
}
const encoder = new TextEncoder();
const decoder = new TextDecoder();
export class EngineError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'EngineError';
  }
}
export function check(e: Exports, memory: Memory, ok: number): void {
  if (!ok)
    throw new EngineError(
      memory.readText(e.imprenta_error_ptr(), e.imprenta_error_len()) ||
        'the receipt engine failed',
    );
}

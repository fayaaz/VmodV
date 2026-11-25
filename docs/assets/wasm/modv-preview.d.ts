/* tslint:disable */
/* eslint-disable */

/**
 * JavaScript-facing preview runtime.
 */
export class PreviewEngine {
    free(): void;
    [Symbol.dispose](): void;
    bass(): number;
    /**
     * 160-byte `ShaderUniforms` block for built-in WGSL modules.
     */
    builtin_uniform_bytes(): Uint8Array;
    /**
     * Variable-size ISF uniform block for the given layout and prop defaults.
     */
    isf_uniform_bytes(layout: string, props: string): Uint8Array;
    kick(): boolean;
    mid(): number;
    constructor(width: number, height: number, bpm: number);
    /**
     * 16-byte-slot prop buffer for built-in WGSL modules.
     */
    prop_bytes(props: string): Uint8Array;
    resize(width: number, height: number): void;
    set_time(seconds: number): void;
    tick(dt_seconds: number): void;
    treble(): number;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_previewengine_free: (a: number, b: number) => void;
    readonly previewengine_bass: (a: number) => number;
    readonly previewengine_builtin_uniform_bytes: (a: number) => [number, number];
    readonly previewengine_isf_uniform_bytes: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly previewengine_kick: (a: number) => number;
    readonly previewengine_mid: (a: number) => number;
    readonly previewengine_new: (a: number, b: number, c: number) => number;
    readonly previewengine_prop_bytes: (a: number, b: number, c: number) => [number, number, number, number];
    readonly previewengine_resize: (a: number, b: number, c: number) => void;
    readonly previewengine_set_time: (a: number, b: number) => void;
    readonly previewengine_tick: (a: number, b: number) => void;
    readonly previewengine_treble: (a: number) => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;

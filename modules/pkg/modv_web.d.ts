/* tslint:disable */
/* eslint-disable */

/**
 * Ask for microphone permission and route the stream into an analyser.
 *
 * The returned promise resolves once the AudioContext is running. A denied
 * permission rejects with the browser's error; the caller keeps silence.
 */
export function enable_microphone(device_id?: string | null): Promise<any>;

/**
 * Start the browser shell on `canvas_id`.
 *
 * `web/main.js` creates the canvas once, then calls this after the wasm
 * module is initialised.
 */
export function start(canvas_id: string): Promise<void>;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly enable_microphone: (a: number, b: number) => any;
    readonly start: (a: number, b: number) => any;
    readonly wasm_bindgen_d97efba495851cf7___convert__closures_____invoke___js_sys_ed9f4ceb25613094___Function_fn_wasm_bindgen_d97efba495851cf7___JsValue_____wasm_bindgen_d97efba495851cf7___sys__Undefined___js_sys_ed9f4ceb25613094___Function_fn_wasm_bindgen_d97efba495851cf7___JsValue_____wasm_bindgen_d97efba495851cf7___sys__Undefined_______true_: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen_d97efba495851cf7___convert__closures_____invoke___wasm_bindgen_d97efba495851cf7___JsValue__core_d941ad8722425d2e___result__Result_____wasm_bindgen_d97efba495851cf7___JsError___true_: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen_d97efba495851cf7___convert__closures_____invoke___wasm_bindgen_d97efba495851cf7___sys__JsNullable_wgpu_983e465604d6a32f___backend__webgpu__webgpu_sys__gen_GpuError__GpuError___core_d941ad8722425d2e___result__Result_____wasm_bindgen_d97efba495851cf7___JsError___true_: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen_d97efba495851cf7___convert__closures_____invoke___wasm_bindgen_d97efba495851cf7___sys__JsNullable_wgpu_983e465604d6a32f___backend__webgpu__webgpu_sys__gen_GpuError__GpuError___core_d941ad8722425d2e___result__Result_____wasm_bindgen_d97efba495851cf7___JsError___true__8: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen_d97efba495851cf7___convert__closures_____invoke___wasm_bindgen_d97efba495851cf7___sys__JsNullable_wgpu_983e465604d6a32f___backend__webgpu__webgpu_sys__gen_GpuError__GpuError___core_d941ad8722425d2e___result__Result_____wasm_bindgen_d97efba495851cf7___JsError___true__9: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen_d97efba495851cf7___convert__closures_____invoke___core_d941ad8722425d2e___result__Result_____wasm_bindgen_d97efba495851cf7___JsValue___true_: (a: number, b: number) => [number, number];
    readonly wasm_bindgen_d97efba495851cf7___convert__closures_____invoke___js_sys_ed9f4ceb25613094___Array______true_: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_d97efba495851cf7___convert__closures_____invoke___js_sys_ed9f4ceb25613094___Array______true__5: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_d97efba495851cf7___convert__closures_____invoke___wgpu_983e465604d6a32f___backend__webgpu__webgpu_sys__gen_GpuDeviceLostInfo__GpuDeviceLostInfo______true_: (a: number, b: number, c: any) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
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

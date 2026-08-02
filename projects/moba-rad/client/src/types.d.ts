/// <reference types="vite/client" />

declare module 'moba-rad/rad-sources' {
  export const radSources: {
    readonly components: string;
    readonly scene: string;
    readonly avatars: string;
    readonly movement: string;
    readonly client: string;
  };
}

declare module '*rad_vm.js' {
  export interface InitOutput {
    readonly memory: WebAssembly.Memory;
  }

  export class RadRuntime {
    runtime_features(): string;
    get_world_snapshot(): string;
    session_start(source: string): string;
    session_emit(event: string, fields_json: string): void;
    session_pump(): string;
    session_render_delta(): string;
    session_render_buffer_refresh(): void;
    session_render_buffer_ptr(): number;
    session_render_buffer_f32_len(): number;
  }

  export default function init(input?: URL | RequestInfo | Response | BufferSource | WebAssembly.Module): Promise<InitOutput>;
}

/// <reference types="vite/client" />

declare module '*rad_vm.js' {
  export interface InitOutput {
    readonly memory: WebAssembly.Memory;
  }

  export class RadRuntime {
    runtime_features(): string;
    session_start(source: string): string;
    session_emit(event: string, fieldsJson: string): void;
    session_pump(): string;
    session_render_buffer_refresh_bounded(maxRecords: number, maxEntitiesScanned: number): void;
    session_render_buffer_ptr(): number;
    session_render_buffer_u32_len(): number;
  }

  export default function init(input?: URL | RequestInfo | Response | BufferSource | WebAssembly.Module): Promise<InitOutput>;
}

interface RadWebGpuDogfoodSnapshot {
  readonly canvasWidth: number;
  readonly deviceEpoch: number;
  readonly errors: readonly string[];
  readonly recordCount: number;
  readonly renderedFrames: number;
  readonly sequence: string;
  readonly streamId: string;
}

interface RadWebGpuDogfoodHarness {
  loseDevice(): void;
  restart(): void;
  settle(): Promise<void>;
  snapshot(): RadWebGpuDogfoodSnapshot;
}

declare var __radWebGpuDogfood: RadWebGpuDogfoodHarness | undefined;

// The packed Float32 render-buffer contract shared between the RAD wasm VM
// (producer) and the browser host (consumer). Kept wasm-free so it can be unit
// tested in Node without loading the VM.
//
// Layout (f32): [version, stride, count, <count records>]
// Each record (stride 9): [entity_id, player_id, x, y, target_x, target_y,
//                          target_active, command_id, model_code]

export const RENDER_BUFFER_VERSION = 1;
export const RENDER_BUFFER_HEADER_F32 = 3;
export const RENDER_BUFFER_STRIDE_F32 = 9;

// RAD entity ids are 0-based, so id 0 is a real entity — specifically the
// first-allocated one, which for the client is the locally-seeded champion
// (`seed_moba_world()` -> player_1 -> entity 0). The buffer only ever holds
// `count` valid records (no padding), so only genuinely malformed ids
// (negative / non-finite from a corrupt read) are rejected — never 0. A prior
// `entity_id <= 0` guard silently dropped the local champion, which then froze
// at spawn while the higher-id server ghost kept rendering.
export function isRenderableEntityId(entityId: number): boolean {
  return Number.isFinite(entityId) && entityId >= 0;
}

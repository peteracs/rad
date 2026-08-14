import type { AvatarPresentationDescriptor } from './contract.js';

/** Builds WGSL from the runtime-owned record descriptor. */
export function createAvatarShader(descriptor: AvatarPresentationDescriptor): string {
  const field = descriptor.fields;
  return /* wgsl */ `
struct ViewUniform {
  world_width: f32,
  world_height: f32,
  radius: f32,
  _padding: f32,
}

struct VertexOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec3<f32>,
}

@group(0) @binding(0) var<storage, read> records: array<u32>;
@group(0) @binding(1) var<uniform> view: ViewUniform;

const RECORD_WORDS: u32 = ${descriptor.recordWords}u;
const X_WORD: u32 = ${field.x}u;
const Y_WORD: u32 = ${field.y}u;
const PLAYER_WORD: u32 = ${field.player_id}u;
const MODEL_WORD: u32 = ${field.model_id}u;

@vertex
fn vertex_main(
  @builtin(vertex_index) vertex_index: u32,
  @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
  let corners = array<vec2<f32>, 6>(
    vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
    vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0),
  );
  let base = instance_index * RECORD_WORDS;
  let center = vec2(
    bitcast<f32>(records[base + X_WORD]),
    bitcast<f32>(records[base + Y_WORD]),
  );
  let half_world = vec2(view.world_width, view.world_height) * 0.5;
  let clip_center = vec2(center.x / half_world.x, -center.y / half_world.y);
  let clip_radius = vec2(view.radius / half_world.x, view.radius / half_world.y);
  let player = records[base + PLAYER_WORD];
  let model = records[base + MODEL_WORD];
  let palette = array<vec3<f32>, 6>(
    vec3(0.31, 0.64, 1.0), vec3(1.0, 0.38, 0.53), vec3(0.61, 0.91, 0.42),
    vec3(0.88, 0.68, 0.29), vec3(0.73, 0.53, 0.96), vec3(0.33, 0.85, 0.78),
  );
  var output: VertexOutput;
  output.position = vec4(clip_center + corners[vertex_index] * clip_radius, 0.0, 1.0);
  output.color = palette[(player + model) % 6u];
  return output;
}

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
  return vec4(input.color, 1.0);
}
`;
}

// The reusable packet contract lives with the WebGPU host. This MOBA adapter
// only adds names that describe how its world-view cache consumes the packet.
export {
  parseAvatarDescriptor,
  parseAvatarPacket,
  signedI64AsSafeNumber,
  type AvatarPresentationDescriptor,
  type AvatarPacketHeader,
  type AvatarPresentationPacket,
} from '../../../../rad-webgpu/src/contract.js';
export {
  WasmAvatarPresentationSource,
  type RadPresentationRuntime,
} from '../../../../rad-webgpu/src/source.js';

export { isU32 as isRenderableEntityId } from '../../../../rad-webgpu/src/contract.js';

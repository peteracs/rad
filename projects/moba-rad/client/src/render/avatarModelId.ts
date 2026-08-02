// THREE-free avatar model identity.
//
// Kept separate from `avatarModels.ts` (which builds Three.js meshes) so pure
// render-state code such as `worldView.ts` and its unit tests never drag the
// 3D engine into their module graph. As the roster grows, the union of valid
// model ids can be typed here without coupling to any rendering code.
export const DEFAULT_AVATAR_MODEL = 'clockwork_mage';

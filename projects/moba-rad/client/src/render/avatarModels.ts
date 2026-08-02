import * as THREE from 'three';
import { createClockworkMageModel } from '../characters/clockworkMage/models/clockworkMageModel';
import { DEFAULT_AVATAR_MODEL } from './avatarModelId';

export { DEFAULT_AVATAR_MODEL } from './avatarModelId';

type AvatarModelFactory = () => THREE.Group;

const AVATAR_MODELS: Record<string, AvatarModelFactory> = {
  clockwork_mage: createClockworkMageModel,
};

export function createAvatarModel(model = DEFAULT_AVATAR_MODEL): THREE.Group {
  const factory = AVATAR_MODELS[model] ?? AVATAR_MODELS[DEFAULT_AVATAR_MODEL];
  const avatar = factory();
  avatar.name = `Avatar:${model}`;
  return avatar;
}

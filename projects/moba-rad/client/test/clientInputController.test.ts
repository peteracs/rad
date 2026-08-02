import { strict as assert } from 'node:assert';
import test from 'node:test';
import {
  ClientInputController,
  type ClientInputControllerCallbacks,
} from '../src/app/clientInputController.js';

// The controller talks to `window` and the canvas purely through the
// EventTarget contract, so Node's real EventTarget stands in for both and
// dispatch semantics stay genuine. Each test installs a fresh window target
// (the module resolves the global lazily inside bind/dispose), which isolates
// listener registrations between tests.

class CallbackRecorder implements ClientInputControllerCallbacks {
  resizes = 0;
  aimCancels = 0;
  readonly moves: { x: number; y: number }[] = [];
  readonly aimPreviews: { x: number; y: number }[] = [];
  readonly casts: { x: number; y: number }[] = [];
  readonly debugToggles: boolean[] = [];

  onResize(): void {
    this.resizes += 1;
  }

  onMoveCommand(clientX: number, clientY: number): void {
    this.moves.push({ x: clientX, y: clientY });
  }

  onAimPreview(clientX: number, clientY: number): void {
    this.aimPreviews.push({ x: clientX, y: clientY });
  }

  onAimCancel(): void {
    this.aimCancels += 1;
  }

  onCastCommand(clientX: number, clientY: number): void {
    this.casts.push({ x: clientX, y: clientY });
  }

  onDebugToggle(enabled: boolean): void {
    this.debugToggles.push(enabled);
  }
}

function pointerEvent(type: string, clientX: number, clientY: number, button = 0): Event {
  return Object.assign(new Event(type, { cancelable: true }), { clientX, clientY, button });
}

function keyEvent(type: string, code: string, repeat = false): Event {
  return Object.assign(new Event(type, { cancelable: true }), { code, repeat });
}

function makeController() {
  const windowTarget = new EventTarget();
  (globalThis as { window?: EventTarget }).window = windowTarget;
  const canvas = new EventTarget();
  const callbacks = new CallbackRecorder();
  const controller = new ClientInputController(
    canvas as unknown as HTMLCanvasElement,
    callbacks,
  );
  controller.bind();
  return { windowTarget, canvas, callbacks, controller };
}

test('right-click issues a move command at the pointer; left-click does not', () => {
  const { canvas, callbacks } = makeController();

  canvas.dispatchEvent(pointerEvent('pointerdown', 100, 50, 0));
  assert.equal(callbacks.moves.length, 0);

  canvas.dispatchEvent(pointerEvent('pointerdown', 120, 60, 2));
  assert.deepEqual(callbacks.moves, [{ x: 120, y: 60 }]);
});

test('the canvas context menu is suppressed', () => {
  const { canvas } = makeController();

  const menu = new Event('contextmenu', { cancelable: true });
  canvas.dispatchEvent(menu);
  assert.equal(menu.defaultPrevented, true);
});

test('holding Q previews the skillshot and releasing casts at the last pointer', () => {
  const { windowTarget, canvas, callbacks } = makeController();

  canvas.dispatchEvent(pointerEvent('pointermove', 10, 20));
  assert.equal(callbacks.aimPreviews.length, 0, 'moving without aiming previews nothing');

  windowTarget.dispatchEvent(keyEvent('keydown', 'KeyQ'));
  assert.deepEqual(callbacks.aimPreviews, [{ x: 10, y: 20 }]);

  canvas.dispatchEvent(pointerEvent('pointermove', 30, 40));
  assert.deepEqual(callbacks.aimPreviews, [
    { x: 10, y: 20 },
    { x: 30, y: 40 },
  ]);

  windowTarget.dispatchEvent(keyEvent('keyup', 'KeyQ'));
  assert.equal(callbacks.aimCancels, 1, 'releasing clears the reticle');
  assert.deepEqual(callbacks.casts, [{ x: 30, y: 40 }]);
});

test('aiming without a known pointer cancels instead of previewing or casting', () => {
  const { windowTarget, callbacks } = makeController();

  windowTarget.dispatchEvent(keyEvent('keydown', 'KeyQ'));
  assert.equal(callbacks.aimPreviews.length, 0);
  assert.equal(callbacks.aimCancels, 1);

  windowTarget.dispatchEvent(keyEvent('keyup', 'KeyQ'));
  assert.equal(callbacks.casts.length, 0, 'no pointer, no cast');
  assert.equal(callbacks.aimCancels, 2);

  // A stray keyup with no aim in progress stays inert.
  windowTarget.dispatchEvent(keyEvent('keyup', 'KeyQ'));
  assert.equal(callbacks.aimCancels, 2);
});

test('key repeats and other keys do not start aiming', () => {
  const { windowTarget, canvas, callbacks } = makeController();
  canvas.dispatchEvent(pointerEvent('pointermove', 5, 6));

  windowTarget.dispatchEvent(keyEvent('keydown', 'KeyQ', true));
  assert.equal(callbacks.aimPreviews.length, 0, 'auto-repeat is ignored');

  windowTarget.dispatchEvent(keyEvent('keydown', 'KeyW'));
  windowTarget.dispatchEvent(keyEvent('keyup', 'KeyW'));
  assert.equal(callbacks.aimPreviews.length, 0);
  assert.equal(callbacks.aimCancels, 0);
  assert.equal(callbacks.casts.length, 0);
});

test('debug toggle accepts only an explicit boolean true detail', () => {
  const { windowTarget, callbacks } = makeController();

  windowTarget.dispatchEvent(new CustomEvent('moba-rad-debug-toggle', { detail: { enabled: true } }));
  windowTarget.dispatchEvent(new CustomEvent('moba-rad-debug-toggle', { detail: { enabled: 'yes' } }));
  windowTarget.dispatchEvent(new CustomEvent('moba-rad-debug-toggle'));

  assert.deepEqual(callbacks.debugToggles, [true, false, false]);
});

test('window resize triggers the resize callback', () => {
  const { windowTarget, callbacks } = makeController();

  windowTarget.dispatchEvent(new Event('resize'));
  assert.equal(callbacks.resizes, 1);
});

test('dispose unbinds every listener', () => {
  const { windowTarget, canvas, callbacks, controller } = makeController();
  canvas.dispatchEvent(pointerEvent('pointermove', 1, 2));

  controller.dispose();

  canvas.dispatchEvent(pointerEvent('pointerdown', 9, 9, 2));
  windowTarget.dispatchEvent(keyEvent('keydown', 'KeyQ'));
  windowTarget.dispatchEvent(keyEvent('keyup', 'KeyQ'));
  windowTarget.dispatchEvent(new Event('resize'));
  windowTarget.dispatchEvent(new CustomEvent('moba-rad-debug-toggle', { detail: { enabled: true } }));

  assert.equal(callbacks.moves.length, 0);
  assert.equal(callbacks.aimPreviews.length, 0);
  assert.equal(callbacks.aimCancels, 0);
  assert.equal(callbacks.casts.length, 0);
  assert.equal(callbacks.resizes, 0);
  assert.equal(callbacks.debugToggles.length, 0);
});

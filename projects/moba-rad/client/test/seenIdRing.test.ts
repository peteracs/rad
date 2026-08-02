import { strict as assert } from 'node:assert';
import test from 'node:test';
import { SeenIdRing } from '../src/netcode/seenIdRing.js';

test('seen id ring remembers non-zero ids and treats zero as already seen', () => {
  const ring = new SeenIdRing(4);

  assert.equal(ring.has(0), true);
  assert.equal(ring.rememberIfNew(0), false);
  assert.equal(ring.rememberIfNew(10), true);
  assert.equal(ring.has(10), true);
  assert.equal(ring.rememberIfNew(10), false);
});

test('seen id ring evicts oldest ids after wrapping capacity', () => {
  const ring = new SeenIdRing(2);

  assert.equal(ring.rememberIfNew(1), true);
  assert.equal(ring.rememberIfNew(2), true);
  assert.equal(ring.rememberIfNew(3), true);
  assert.equal(ring.has(1), false);
  assert.equal(ring.has(2), true);
  assert.equal(ring.has(3), true);
});

test('seen id ring requires power-of-two capacity for mask-based wrapping', () => {
  assert.throws(() => new SeenIdRing(3), /power of two/);
  assert.throws(() => new SeenIdRing(0), /power of two/);
});

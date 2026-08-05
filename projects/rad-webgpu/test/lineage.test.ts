import { strict as assert } from 'node:assert';
import { test } from 'node:test';

import { PresentationLineage } from '../src/lineage.js';
import { packetHeader } from './fixtures.js';

test('new streams require an initial full packet and reset the mirror', () => {
  const lineage = new PresentationLineage();
  assert.equal(lineage.inspect(packetHeader()).resetMirror, true);
  lineage.commit(packetHeader());

  const nextStream = packetHeader({ streamId: 2n });
  assert.equal(lineage.inspect(nextStream).resetMirror, true);
  assert.throws(
    () => lineage.inspect(packetHeader({ streamId: 3n, packetKind: 'delta' })),
    /new_stream_requires_full/,
  );
  assert.throws(
    () => lineage.inspect(packetHeader({ streamId: 3n, sequence: 1n })),
    /new_stream_sequence_not_zero/,
  );
});

test('deltas extend exactly one accepted stream baseline', () => {
  const lineage = new PresentationLineage();
  lineage.commit(packetHeader());
  const delta = packetHeader({
    sequence: 1n,
    packetKind: 'delta',
    baseSequence: 0n,
  });
  assert.equal(lineage.inspect(delta).resetMirror, false);
  lineage.commit(delta);

  assert.throws(() => lineage.inspect(delta), /stale_sequence/);
  assert.throws(
    () => lineage.inspect(packetHeader({
      sequence: 3n,
      packetKind: 'delta',
      baseSequence: 1n,
    })),
    /delta_sequence_gap/,
  );
  assert.throws(
    () => lineage.inspect(packetHeader({
      sequence: 2n,
      packetKind: 'delta',
      baseSequence: 0n,
    })),
    /delta_base_mismatch/,
  );
});

test('device invalidation requires a full baseline without changing stream identity', () => {
  const lineage = new PresentationLineage();
  lineage.commit(packetHeader());
  lineage.invalidateBaseline();
  assert.throws(
    () => lineage.inspect(packetHeader({
      sequence: 1n,
      packetKind: 'delta',
      baseSequence: 0n,
    })),
    /delta_without_baseline/,
  );
  assert.equal(lineage.inspect(packetHeader({ sequence: 2n })).resetMirror, true);
});

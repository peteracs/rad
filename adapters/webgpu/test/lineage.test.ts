import { strict as assert } from 'node:assert';
import { test } from 'node:test';

import { PresentationLineage } from '../src/presentation/lineage.js';
import { packetHeader } from './fixtures.js';

test('new streams require an initial full packet at sequence zero', () => {
  const lineage = new PresentationLineage();
  lineage.accept(packetHeader());
  lineage.accept(packetHeader({ streamId: 2n }));
  assert.throws(
    () => lineage.accept(packetHeader({ streamId: 3n, packetKind: 'delta' })),
    /new_stream_requires_full/,
  );
  assert.throws(
    () => lineage.accept(packetHeader({ streamId: 3n, sequence: 1n })),
    /new_stream_sequence_not_zero/,
  );
});

test('deltas extend exactly one accepted logical publication', () => {
  const lineage = new PresentationLineage();
  lineage.accept(packetHeader());
  const delta = packetHeader({
    sequence: 1n,
    packetKind: 'delta',
    baseSequence: 0n,
  });
  lineage.accept(delta);
  assert.throws(() => lineage.accept(delta), /stale_sequence/);
  assert.throws(
    () => lineage.accept(packetHeader({
      sequence: 3n,
      packetKind: 'delta',
      baseSequence: 1n,
    })),
    /delta_sequence_gap/,
  );
  assert.throws(
    () => lineage.accept(packetHeader({
      sequence: 2n,
      packetKind: 'delta',
      baseSequence: 0n,
    })),
    /delta_base_mismatch/,
  );
});

test('logical packet lineage is independent from GPU device epochs', () => {
  const lineage = new PresentationLineage();
  lineage.accept(packetHeader());
  lineage.accept(packetHeader({
    sequence: 1n,
    packetKind: 'delta',
    baseSequence: 0n,
  }));
});

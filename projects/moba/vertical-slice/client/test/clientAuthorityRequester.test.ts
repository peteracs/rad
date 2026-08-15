import { strict as assert } from 'node:assert';
import test from 'node:test';
import { ClientAuthorityRequester } from '../src/app/clientAuthorityRequester.js';
import { ClientNetcodeTelemetry } from '../src/app/clientNetcodeTelemetry.js';
import { AckDiagnostics } from '../src/netcode/ackDiagnostics.js';
import { INPUT_DELAY_TICKS, MAX_INPUT_DELAY_TICKS } from '../src/netcode/constants.js';
import { ClientInputSequencer } from '../src/netcode/inputSequencer.js';
import { createNetcodeDiagnosticsSnapshot } from '../src/netcode/runtimeDiagnostics.js';
import type { ServerState } from '../src/transport/serverState.js';
import { FakeMatchTransport, makeServerState } from './appTestDoubles.js';

function makeRequester() {
  const transport = new FakeMatchTransport();
  const inputSequencer = new ClientInputSequencer();
  const ackDiagnostics = new AckDiagnostics(INPUT_DELAY_TICKS, MAX_INPUT_DELAY_TICKS);
  const telemetry = new ClientNetcodeTelemetry();
  const requester = new ClientAuthorityRequester(
    transport,
    inputSequencer,
    ackDiagnostics,
    telemetry,
  );
  return { requester, transport, inputSequencer, telemetry };
}

function telemetrySnapshot(telemetry: ClientNetcodeTelemetry) {
  return telemetry.writeSnapshot(createNetcodeDiagnosticsSnapshot(), 0);
}

test('request resolves the transport state with a freshly reserved client seq', async () => {
  const { requester, transport, telemetry } = makeRequester();
  const state = makeServerState({ serverTick: 5 });
  transport.stateHandler = () => Promise.resolve(state);

  const request = requester.request(0);
  assert.ok(request, 'request is issued');
  assert.equal(requester.inFlight, true);

  assert.equal(await request, state);
  assert.equal(requester.inFlight, false);
  assert.deepEqual(transport.stateRequests, [1], 'first reserved client seq rides along');
  assert.equal(telemetrySnapshot(telemetry).authorityStateRequests, 1);
});

test('only one authority request is in flight at a time', async () => {
  const { requester, transport } = makeRequester();
  let resolveState!: (state: ServerState) => void;
  transport.stateHandler = () => new Promise<ServerState>((resolve) => {
    resolveState = resolve;
  });

  const first = requester.request(0);
  assert.ok(first);
  assert.equal(requester.request(0), null, 'second request rejected while pending');
  assert.equal(transport.stateRequests.length, 1);

  resolveState(makeServerState());
  await first;
  assert.equal(requester.inFlight, false);
});

test('maybeRequest polls only when enabled and the poll interval elapsed', async () => {
  const { requester, transport } = makeRequester();
  transport.stateHandler = () => Promise.resolve(makeServerState());

  assert.equal(requester.maybeRequest(0, false), null, 'disabled polling stays quiet');

  const first = requester.maybeRequest(0, true);
  assert.ok(first);
  await first;

  assert.equal(requester.maybeRequest(99, true), null, 'poll window is 100ms');
  const second = requester.maybeRequest(100, true);
  assert.ok(second);
  await second;
  assert.equal(transport.stateRequests.length, 2);
});

test('transport failures resolve to null and classify timeouts', async () => {
  const { requester, transport, telemetry } = makeRequester();

  transport.stateHandler = () => Promise.reject(new Error('boom'));
  assert.equal(await requester.request(0), null);
  assert.equal(requester.inFlight, false, 'failure clears the in-flight latch');
  let diag = telemetrySnapshot(telemetry);
  assert.equal(diag.transportFailures, 1);
  assert.equal(diag.lastTransportError, 'boom');
  assert.equal(diag.authorityTimeouts, 0);

  transport.stateHandler = () => Promise.reject(new Error('Timed out waiting for authority state'));
  assert.equal(await requester.request(200), null);
  diag = telemetrySnapshot(telemetry);
  assert.equal(diag.transportFailures, 2);
  assert.equal(diag.authorityTimeouts, 1);
});

test('closed requester never contacts the transport again', () => {
  const { requester, transport } = makeRequester();
  transport.stateHandler = () => Promise.resolve(makeServerState());

  requester.close();
  assert.equal(requester.request(0), null);
  assert.equal(requester.maybeRequest(500, true), null);
  assert.equal(transport.stateRequests.length, 0);
});

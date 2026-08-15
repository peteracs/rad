import './style.css';
import { createMatchIdentity } from './app/matchIdentity';
import { MobaRadClient } from './app/MobaRadClient';
import { RadGameSession } from './radHost';
import { MobaRadWebTransport } from './transport/webTransport';
import { createNetcodeHud } from './ui/netcodeHud';

const canvas = document.querySelector<HTMLCanvasElement>('#scene');
if (!canvas) throw new Error('missing #scene canvas');

const identity = createMatchIdentity();
const session = await RadGameSession.create(identity.playerId);
const authorityTransport = new MobaRadWebTransport(identity);
const netcodeLoggerEnabled = import.meta.env.VITE_MOBA_RAD_NETCODE_LOG === '1';
const client = new MobaRadClient({
  canvas,
  identity,
  session,
  transport: authorityTransport,
  netcodeLogger: netcodeLoggerEnabled ? { enabled: true } : undefined,
});
const netcodeHud = createNetcodeHud(document.querySelector<HTMLElement>('#netcode-hud'), {
  mini: document.querySelector<HTMLElement>('#net-mini'),
  onDebugVisibilityChange: (enabled) => {
    window.dispatchEvent(new CustomEvent('moba-rad-debug-toggle', { detail: { enabled } }));
  },
});
const reconciliationFlash = document.querySelector<HTMLElement>('#snap-flash');
let reconciliationFlashTimer = 0;

window.addEventListener('moba-rad-hard-correction', () => {
  if (!reconciliationFlash) return;
  reconciliationFlash.classList.add('snap-flash--on');
  if (reconciliationFlashTimer !== 0) window.clearTimeout(reconciliationFlashTimer);
  reconciliationFlashTimer = window.setTimeout(() => {
    reconciliationFlash.classList.remove('snap-flash--on');
    reconciliationFlashTimer = 0;
  }, 180);
});
client.start();
netcodeHud.start(client);
window.addEventListener('beforeunload', () => {
  if (reconciliationFlashTimer !== 0) window.clearTimeout(reconciliationFlashTimer);
  netcodeHud.stop();
  client.dispose();
}, { once: true });

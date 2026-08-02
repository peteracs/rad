import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath, URL } from 'node:url';
import { defineConfig, type Plugin } from 'vite';

const RAD_SOURCES_MODULE_ID = 'moba-rad/rad-sources';
const RESOLVED_RAD_SOURCES_MODULE_ID = `\0${RAD_SOURCES_MODULE_ID}`;

const ISOLATION_HEADERS = {
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cross-Origin-Embedder-Policy': 'require-corp',
};

export default defineConfig({
  plugins: [radSourcesPlugin(), webTransportCertHashPlugin()],
  build: {
    target: 'esnext',
    sourcemap: false,
  },
  server: {
    host: '127.0.0.1',
    port: 5174,
    hmr: false,
    headers: ISOLATION_HEADERS,
    fs: {
      allow: [fileURLToPath(new URL('../..', import.meta.url))],
    },
  },
  preview: {
    host: '127.0.0.1',
    port: 4174,
    headers: ISOLATION_HEADERS,
  },
});

function radSourcesPlugin(): Plugin {
  const files = {
    components: fileURLToPath(new URL('../server/src/sim/components.rad', import.meta.url)),
    scene: fileURLToPath(new URL('../server/src/world/scene.rad', import.meta.url)),
    avatars: fileURLToPath(new URL('../server/src/world/avatars.rad', import.meta.url)),
    movement: fileURLToPath(new URL('../server/src/sim/movement.rad', import.meta.url)),
    client: fileURLToPath(new URL('./src/rad/main.rad', import.meta.url)),
  };
  const paths = Object.values(files);

  return {
    name: 'moba-rad-sources',
    resolveId(id: string) {
      if (id === RAD_SOURCES_MODULE_ID) return RESOLVED_RAD_SOURCES_MODULE_ID;
      return null;
    },
    load(id: string) {
      if (id !== RESOLVED_RAD_SOURCES_MODULE_ID) return null;
      // Track the shared .rad files so an edit invalidates this virtual module.
      // Without this, the concatenated source is read once and cached, and edits
      // (e.g. a new event/handler) never reach the browser VM until the dev
      // server is fully restarted — surfacing as `session_emit: unknown event`.
      for (const path of paths) this.addWatchFile(path);
      return `export const radSources = ${JSON.stringify({
        components: readFileSync(files.components, 'utf8'),
        scene: readFileSync(files.scene, 'utf8'),
        avatars: readFileSync(files.avatars, 'utf8'),
        movement: readFileSync(files.movement, 'utf8'),
        client: readFileSync(files.client, 'utf8'),
      })};`;
    },
    configureServer(server) {
      const reloadOnRadChange = (file: string) => {
        if (!paths.includes(file)) return;
        const module = server.moduleGraph.getModuleById(RESOLVED_RAD_SOURCES_MODULE_ID);
        if (module) server.moduleGraph.invalidateModule(module);
        // HMR is disabled for this app, so request an explicit full reload. This
        // is a no-op if the ws is unavailable, but the invalidation above still
        // guarantees the NEXT browser refresh recompiles fresh RAD source.
        server.ws.send({ type: 'full-reload' });
      };
      server.watcher.add(paths);
      server.watcher.on('change', reloadOnRadChange);
      server.watcher.on('add', reloadOnRadChange);
    },
  };
}

// Derive the WebTransport cert pin from the SAME cert file the edge proxy
// persists, so the browser-pinned SHA-256 hash never has to be copy-pasted.
// The proxy mints a stable self-signed cert at `<edge-proxy>/.dev-certs`; we
// read it here, hash the DER exactly like Chromium's `serverCertificateHashes`
// path, and inject it as `VITE_MOBA_RAD_WEBTRANSPORT_CERT_HASH`. An explicit
// env var (CI, a browser-trusted cert, or a custom proxy) always wins.
function webTransportCertHashPlugin(): Plugin {
  const ENV_KEY = 'VITE_MOBA_RAD_WEBTRANSPORT_CERT_HASH';
  const certPath = resolveDevCertPath();

  return {
    name: 'moba-rad-webtransport-cert-hash',
    config() {
      if (process.env[ENV_KEY]) return undefined;

      const hash = certHashFromPem(certPath);
      if (!hash) return undefined;

      // eslint-disable-next-line no-console
      console.log(`[moba-rad] pinned WebTransport cert ${certPath}\n           sha-256 ${hash}`);
      return {
        define: {
          [`import.meta.env.${ENV_KEY}`]: JSON.stringify(hash),
        },
      };
    },
  };
}

function resolveDevCertPath(): string {
  const override = process.env.MOBA_RAD_CERT_DIR;
  const dir = override
    ? resolve(process.cwd(), override)
    : fileURLToPath(new URL('../server/edge-proxy/.dev-certs', import.meta.url));
  return join(dir, 'localhost.crt');
}

function certHashFromPem(certPath: string): string | null {
  if (!existsSync(certPath)) return null;

  const pem = readFileSync(certPath, 'utf8');
  const match = pem.match(/-----BEGIN CERTIFICATE-----([\s\S]+?)-----END CERTIFICATE-----/);
  if (!match) return null;

  const der = Buffer.from(match[1].replace(/\s+/g, ''), 'base64');
  return createHash('sha256').update(der).digest('hex');
}

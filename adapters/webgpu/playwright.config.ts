import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './browser-test',
  fullyParallel: false,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? 'github' : 'line',
  timeout: 60_000,
  workers: 1,
  use: {
    baseURL: 'http://127.0.0.1:5175',
    channel: 'chromium',
    // Headed Chromium under Xvfb exercises the production compositor path on
    // Linux. New-headless can tear down its software Dawn instance while a
    // staging-buffer map is still pending.
    headless: !process.env.CI,
    launchOptions: {
      args: [
        '--enable-unsafe-webgpu',
        '--use-webgpu-adapter=swiftshader',
        '--use-vulkan=swiftshader',
        '--enable-features=Vulkan',
        '--enable-dawn-features=allow_unsafe_apis',
        '--disable-dawn-features=use_dxc',
        '--enable-webgpu-developer-features',
        '--use-gpu-in-tests',
        '--enable-accelerated-2d-canvas',
      ],
    },
  },
  webServer: {
    command: 'npm run dev -- --strictPort',
    url: 'http://127.0.0.1:5175',
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});

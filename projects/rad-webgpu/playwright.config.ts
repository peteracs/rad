import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './browser-test',
  fullyParallel: false,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? 'github' : 'line',
  timeout: 60_000,
  use: {
    baseURL: 'http://127.0.0.1:5175',
    channel: 'chromium',
    headless: true,
    launchOptions: {
      args: [
        '--enable-unsafe-webgpu',
        '--use-webgpu-adapter=swiftshader',
        '--use-vulkan=swiftshader',
        '--enable-features=UseSkiaRenderer,Vulkan',
        '--enable-gpu-rasterization',
        '--enable-oop-rasterization',
        '--disable-vulkan-fallback-to-gl-for-testing',
        '--enable-dawn-features=allow_unsafe_apis',
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

import { defineConfig } from "@playwright/test";

// UI end-to-end tests run the Vite build in Chromium with a mocked Tauri IPC
// (e2e/tauri-mock.js). The native core is covered by `cargo test`.
export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  use: { baseURL: "http://localhost:1420", viewport: { width: 1280, height: 820 } },
  webServer: { command: "npx vite --port 1420 --strictPort", url: "http://localhost:1420", reuseExistingServer: true, timeout: 60_000 },
});

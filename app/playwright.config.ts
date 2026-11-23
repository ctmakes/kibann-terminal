import { defineConfig, devices } from "@playwright/test";

// Chromium stands in for WebView2 (Windows); WebKit for WKWebView (macOS) and
// WebKitGTK (Linux). Each runs at 1x and 2x device pixel ratios.
const engines = [
  ["chromium", devices["Desktop Chrome"]],
  ["webkit", devices["Desktop Safari"]],
] as const;

export default defineConfig({
  testDir: "e2e",
  snapshotPathTemplate: "{testDir}/__screenshots__/{projectName}/{arg}{ext}",
  fullyParallel: true,
  reporter: [["list"]],
  expect: {
    // Text antialiasing differs slightly between machines and GPU paths.
    toHaveScreenshot: { maxDiffPixelRatio: 0.002, threshold: 0.15 },
  },
  use: { baseURL: "http://127.0.0.1:1420" },
  projects: [
    ...engines.flatMap(([name, device]) =>
      [1, 2].map((dpr) => ({
        name: `${name}-${dpr}x`,
        use: { ...device, deviceScaleFactor: dpr, viewport: { width: 1280, height: 800 } },
      })),
    ),
    // The built pages under the release CSP (vite.config.ts): the suite's
    // behaviour, with every violation failing the run (csp-teardown.ts).
    // Dev serves CSS as <style> elements, which that CSP would block.
    {
      name: "webkit-csp",
      testIgnore: /bench/,
      ignoreSnapshots: true,
      use: { ...devices["Desktop Safari"], baseURL: "http://127.0.0.1:1421", viewport: { width: 1280, height: 800 } },
    },
  ],
  globalTeardown: "./e2e/csp-teardown.ts",
  webServer: [
    {
      command: "npx vite --port 1420 --host 127.0.0.1 --strictPort",
      url: "http://127.0.0.1:1420/src/dev/harness.html",
      reuseExistingServer: true,
    },
    {
      command: "npx vite build --mode csp --outDir dist-csp --emptyOutDir --logLevel warn && npx vite preview --mode csp --outDir dist-csp",
      url: "http://127.0.0.1:1421/src/dev/harness.html",
      reuseExistingServer: false,
      timeout: 120_000,
    },
  ],
});

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
  use: { baseURL: "http://localhost:5173" },
  projects: engines.flatMap(([name, device]) =>
    [1, 2].map((dpr) => ({
      name: `${name}-${dpr}x`,
      use: { ...device, deviceScaleFactor: dpr, viewport: { width: 1280, height: 800 } },
    })),
  ),
  webServer: {
    command: "npx vite --port 5173 --strictPort",
    url: "http://localhost:5173/src/dev/harness.html",
    reuseExistingServer: true,
  },
});

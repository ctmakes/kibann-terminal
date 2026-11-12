import { resolve } from "node:path";
import { defineConfig } from "vitest/config";

export default defineConfig({
  // Tauri expects a fixed port and no clearing of its log output.
  server: { port: 1420, strictPort: true, host: "127.0.0.1" },
  clearScreen: false,
  build: {
    target: "es2022",
    rollupOptions: {
      input: {
        main: resolve(import.meta.dirname, "index.html"),
        harness: resolve(import.meta.dirname, "src/dev/harness.html"),
      },
    },
  },
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});

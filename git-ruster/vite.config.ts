import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [preact()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
    chunkSizeWarningLimit: 4000,
  },
});

import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;
const proxy = {
  "/api": "http://127.0.0.1:8888",
  "/ws": {
    target: "ws://127.0.0.1:8888",
    ws: true,
  },
};

export default defineConfig({
  root: "frontend",
  build: {
    outDir: "../src",
    emptyOutDir: true,
  },
  server: {
    host: host || "127.0.0.1",
    port: 1420,
    strictPort: true,
    proxy,
  },
  preview: {
    host: "127.0.0.1",
    port: 4173,
    strictPort: true,
    proxy,
  },
});

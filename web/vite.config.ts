import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  build: {
    // The Rust binary serves this directory via tower-http ServeDir.
    outDir: "dist",
  },
  server: {
    // Only used in `npm run dev`. In production the Rust binary serves the
    // bundle and the socket from the same origin, so there is nothing to proxy.
    proxy: {
      "/api": { target: "http://localhost:8080", changeOrigin: true },
      "/ws": { target: "ws://localhost:8080", ws: true },
    },
  },
});

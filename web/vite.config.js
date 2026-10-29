import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// In dev, proxy API + media routes to the Rust backend on :8080.
// In production the Rust server serves the built `dist/` itself, so these
// relative paths resolve against the same origin.
export default defineConfig({
  plugins: [react()],
  // Relative asset URLs, so the same build works at the root or under a
  // sub-path (the server's URL_BASE -- see src/spa.rs).
  base: "./",
  build: { outDir: "dist" },
  server: {
    port: 5173,
    proxy: {
      "/api": "http://localhost:8080",
      "/healthz": "http://localhost:8080",
    },
  },
});

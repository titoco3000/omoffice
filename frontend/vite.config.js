import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// The API base URL is not read from a .env file: the Makefile sources
// .env.<stage> and exports VITE_API_URL, exactly like backend/Makefile does.
// That keeps `make run local` from colliding with Vite's own `.env.local`,
// which Vite loads in every mode.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  // amazon-chime-sdk-js tests for browser APIs with node's `global`
  // (`'document' in global`), which no bundler supplies in the browser; without
  // this, constructing a meeting session throws "global is not defined".
  // Both spellings are needed: `define` covers the production build, the
  // optimizeDeps one covers the dependency esbuild pre-bundles for `vite dev`.
  define: { global: "globalThis" },
  optimizeDeps: {
    esbuildOptions: { define: { global: "globalThis" } },
  },
  server: {
    port: Number(process.env.PORT ?? 5175),
    strictPort: true,
  },
});

import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// The API base URL is not read from a .env file: the Makefile sources
// .env.<stage> and exports VITE_API_URL, exactly like backend/Makefile does.
// That keeps `make run local` from colliding with Vite's own `.env.local`,
// which Vite loads in every mode.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: Number(process.env.PORT ?? 5173),
    strictPort: true,
  },
});

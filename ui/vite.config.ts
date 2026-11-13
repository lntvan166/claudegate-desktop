import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // Our own icons and Monaco only: nothing is fetched at runtime.
  // Monaco's core editor is one ~3 MB chunk, loaded lazily when the first diff opens.
  build: { assetsInlineLimit: 0, chunkSizeWarningLimit: 3200 },
});

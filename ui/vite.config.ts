import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // Our own icons and Monaco only: nothing is fetched at runtime.
  build: { assetsInlineLimit: 0 },
});

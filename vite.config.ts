import { defineConfig } from "vite";

// Config Vite pour Tauri : port fixe, pas de clearScreen pour garder les logs Rust visibles
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "chrome105",
    minify: "esbuild",
    sourcemap: false,
  },
});

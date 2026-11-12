import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Porta fixa exigida pelo Tauri (devUrl em tauri.conf.json).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // Não vigiar a pasta de compilação do Rust (evita EBUSY no Windows).
    watch: { ignored: ["**/src-tauri/**"] },
  },
});

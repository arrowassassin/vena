import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Dev: proxy /api to the vena-devserver (real engine). Tauri prod uses invoke().
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: { "/api": "http://127.0.0.1:5714" },
  },
  build: { outDir: "dist", sourcemap: false },
});

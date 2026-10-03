import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// exe に埋め込む（tauri.conf.json の frontendDist: ui/dist）。相対パスで読めるように base は "./"
export default defineConfig({
  plugins: [react()],
  base: "./",
  build: { outDir: "dist", emptyOutDir: true },
});

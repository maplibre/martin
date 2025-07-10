import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// https://vitejs.dev/config/
export default defineConfig({
  build: {
    // assets can also be the name of a tile source
    // so we use /_/assets to avoid conflicts
    assetsDir: "_/assets",
  },
  plugins: [react()],
});

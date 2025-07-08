import path from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// https://vitejs.dev/config/
export default defineConfig({
  build: {
    outDir: "dist",
    rollupOptions: {
      output: {
        manualChunks: {
          maplibre: ["maplibre-gl", "@vis.gl/react-maplibre", "@maplibre/maplibre-gl-inspect"],
        },
      },
    },
    sourcemap: true,
  },
  define: {
    "process.env.NODE_ENV": JSON.stringify(process.env.NODE_ENV || "development"),
  },
  envPrefix: "VITE_",
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    port: 3001,
  },
});

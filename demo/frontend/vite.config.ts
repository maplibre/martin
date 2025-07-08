import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import mkcert from "vite-plugin-mkcert";
import viteTsConfigPaths from "vite-tsconfig-paths";

export default defineConfig({
  build: {
    target: "esnext",
  },
  plugins: [
    react(),
    viteTsConfigPaths({
      root: "./",
    }),
    mkcert(),
  ],
  server: { host: true, https: false, port: 8080 },
});

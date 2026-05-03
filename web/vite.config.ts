import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const devdUrl = process.env.MAINS_AEGIS_DEVD_URL ?? process.env.VITE_DEVD_API_BASE ?? "http://127.0.0.1:30080";

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": {
        target: devdUrl,
        changeOrigin: true,
      },
      "/events": {
        target: devdUrl,
        changeOrigin: true,
      },
    },
  },
});

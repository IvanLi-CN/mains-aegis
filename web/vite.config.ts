import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": {
        target: process.env.MAINS_AEGIS_DEVD_URL ?? "http://127.0.0.1:30080",
        changeOrigin: true,
      },
      "/events": {
        target: process.env.MAINS_AEGIS_DEVD_URL ?? "http://127.0.0.1:30080",
        changeOrigin: true,
      },
    },
  },
});

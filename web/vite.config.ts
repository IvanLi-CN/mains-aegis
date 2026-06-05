import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const devdUrl = process.env.MAINS_AEGIS_DEVD_URL ?? process.env.VITE_DEFAULT_DEVD_URL ?? process.env.VITE_DEVD_API_BASE ?? "http://127.0.0.1:30080";
const appBase = normalizeBase(process.env.PAGES_BASE ?? process.env.VITE_BASE);

function normalizeBase(base: string | undefined): string {
  const raw = (base ?? "/").trim();
  if (!raw || raw === "/") return "/";
  const withLeading = raw.startsWith("/") ? raw : `/${raw}`;
  return withLeading.endsWith("/") ? withLeading : `${withLeading}/`;
}

export default defineConfig({
  base: appBase,
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

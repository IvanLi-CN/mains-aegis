/// <reference types="vite/client" />

declare global {
  interface ImportMetaEnv {
    readonly VITE_DEVD_API_BASE?: string;
    readonly VITE_DEFAULT_DEVD_URL?: string;
    readonly VITE_APP_RUNTIME_MODE?: string;
    readonly VITE_FIRMWARE_CATALOG_URL?: string;
  }

  interface ImportMeta {
    readonly env: ImportMetaEnv;
  }
}

export {};

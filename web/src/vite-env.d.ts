/// <reference types="vite/client" />

declare global {
  interface ImportMetaEnv {
    readonly VITE_FIRMWARE_CATALOG_URL?: string;
  }

  interface ImportMeta {
    readonly env: ImportMetaEnv;
  }
}

export {};

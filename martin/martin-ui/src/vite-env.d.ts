/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_MARTIN_VERSION?: string
  readonly VITE_MARTIN_BASE?: string
  // more env variables...
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}

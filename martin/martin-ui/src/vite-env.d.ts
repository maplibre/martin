/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_MARTIN_VERSION: string
  // more env variables...
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}

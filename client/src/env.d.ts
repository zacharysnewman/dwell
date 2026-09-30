declare const __BUILD_SHA__: string;
declare const __BUILD_TIME__: string;

interface ImportMetaEnv {
  /** The master server (ARCHITECTURE.md §10.3); set by the Pages build from a repository variable. */
  readonly VITE_MASTER_URL?: string;
}

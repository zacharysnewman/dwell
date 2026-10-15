declare const __BUILD_SHA__: string;
declare const __BUILD_TIME__: string;
/** The app version of this build (RELEASES.md §3); set by vite.config.ts. */
declare const __APP_VERSION__: string;
/** A local build (no DWELL_VERSION): dev-only pages such as `?marchtest=1`; set by vite.config.ts. */
declare const __LOCAL_BUILD__: boolean;
/**
 * The renderer switch (`?renderer=raster|march|compare`, compare mode): true in local builds (dev
 * server, local preview, bench), false in deployed ones, which keep the mesher (vite.config.ts).
 */
declare const __RENDERER_SWITCH__: boolean;

interface ImportMetaEnv {
  /** The master server (ARCHITECTURE.md §10.3); set by the Pages build from a repository variable. */
  readonly VITE_MASTER_URL?: string;
}

/** OPFS sync access handles (workers only; not in TypeScript's DOM lib): local worlds' files. */
interface FileSystemSyncAccessHandle {
  read(buffer: BufferSource, options?: { at?: number }): number;
  write(buffer: BufferSource, options?: { at?: number }): number;
  getSize(): number;
  truncate(size: number): void;
  flush(): void;
  close(): void;
}

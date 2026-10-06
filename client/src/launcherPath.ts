/**
 * The launcher's path (ADR 0005, RELEASES.md §5): `/dwell/` always, whichever version is open.
 * Navigating to it with a query opens the build the choice needs; in a local or preview build,
 * which is served at `/dwell/` itself, it is the same page.
 */
export const LAUNCHER_PATH = '/dwell/';

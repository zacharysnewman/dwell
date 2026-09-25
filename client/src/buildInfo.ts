export interface BuildInfo {
  sha: string;
  time: string;
}

export const buildInfo: BuildInfo = { sha: __BUILD_SHA__, time: __BUILD_TIME__ };

/** One-line label for the build-info overlay, e.g. "dwell 1a2b3c4 · 2026-09-25". */
export function formatBuildInfo(info: BuildInfo): string {
  const sha = /^[0-9a-f]{7,40}$/i.test(info.sha) ? info.sha.slice(0, 7) : info.sha;
  const date = info.time.slice(0, 10);
  return `dwell ${sha} · ${date}`;
}

// The streaming report (debug tooling: F8, the Debug page's button, `__dwell.streamingReport()`;
// docs/DEV_TOOLS.md): the game's picture of the chunk streaming (the near ring's chunks and what
// each waits on, the job pool, the streamer, the LOD, the last seconds' frames;
// crates/dwell-client/src/game/streaming_report.rs) with the build, device, settings and world
// it came from, as one JSON object the owner pastes into a bug report (BUGS.md #54).

/** The device a report came from. */
export interface ReportDevice {
  /** The user agent, cut to its first 160 characters. */
  ua: string;
  hardwareConcurrency: number | null;
  /** GB, where the browser says (Chrome). */
  deviceMemory: number | null;
  /** The GPU adapter's vendor and description, where the browser says. */
  gpu: string | null;
  /** A CPU (software) adapter: the cheapest tier of everything is drawn. */
  cpuAdapter: boolean;
  /** CSS pixels and the device pixel ratio. */
  screen: { width: number; height: number; dpr: number };
}

export interface ReportSettings {
  renderScale: number;
  upscaler: string;
  /** Full detail (m), Distant detail (pixel error), Distant memory (MB). */
  fullDetailM: number;
  distantPixelError: number;
  distantMemoryMb: number;
  /** The graphics preset the settings match, or "custom". */
  preset: string;
}

export interface ReportWorld {
  local: boolean;
  /** A decimal string: it can exceed 2^53. */
  seed: string | null;
  generatorVersion: number | null;
  /** The URL's `world` and `mode` (e.g. flat, sandbox), where given. */
  type: string | null;
  mode: string | null;
  compatibilityLine: string | null;
}

export interface StreamingReportInput {
  build: { version: string; sha: string; channel: string };
  /** The game's part (`Client.streamingReport()`), parsed; null before playing. */
  game: Record<string, unknown> | null;
  device: ReportDevice;
  settings: ReportSettings | null;
  world: ReportWorld;
  /** The fly speed level, while flying. */
  flySpeedLevel?: number;
  time?: Date;
}

export interface StreamingReport {
  /** What this is, for whoever reads a pasted report. */
  kind: 'dwell-streaming-report';
  version: string;
  sha: string;
  channel: string;
  time: string;
  device: ReportDevice;
  settings: ReportSettings | null;
  world: ReportWorld;
  flySpeedLevel?: number;
  [game: string]: unknown;
}

/** The longest a report's text should be (bytes): lists are cut to stay under it. */
export const REPORT_MAX_BYTES = 200_000;

/** The report: the shell's parts first, then the game's. */
export function streamingReport(input: StreamingReportInput): StreamingReport {
  const report: StreamingReport = {
    kind: 'dwell-streaming-report',
    version: input.build.version,
    sha: input.build.sha,
    channel: input.build.channel,
    time: (input.time ?? new Date()).toISOString(),
    device: input.device,
    settings: input.settings,
    world: input.world,
    ...(input.flySpeedLevel === undefined ? {} : { flySpeedLevel: input.flySpeedLevel }),
    ...(input.game ?? { game: null }),
  };
  return report;
}

/**
 * The report as text, indented by two where that fits [`REPORT_MAX_BYTES`], else on one line, and
 * the near ring's chunks drawn in full dropped (counted) if it still does not.
 */
export function formatStreamingReport(report: StreamingReport): string {
  const pretty = JSON.stringify(report, null, 2);
  if (pretty.length <= REPORT_MAX_BYTES) return pretty;
  const flat = JSON.stringify(report);
  if (flat.length <= REPORT_MAX_BYTES) return flat;
  const ring = report.nearRing as { chunks?: { flags?: string[] }[] } | undefined;
  if (ring?.chunks) {
    const all = ring.chunks;
    const kept = all.filter((c) => !(c.flags ?? []).includes('drawn'));
    report.nearRing = { ...ring, chunks: kept, drawnOmitted: all.length - kept.length };
  }
  return JSON.stringify(report);
}

/** The device, from the browser (`gpu` and `cpuAdapter` are the caller's). */
export function deviceOf(gpu: string | null, cpuAdapter: boolean): ReportDevice {
  const nav = navigator as Navigator & { deviceMemory?: number };
  return {
    ua: nav.userAgent.slice(0, 160),
    hardwareConcurrency: nav.hardwareConcurrency || null,
    deviceMemory: nav.deviceMemory ?? null,
    gpu,
    cpuAdapter,
    screen: { width: screen.width, height: screen.height, dpr: devicePixelRatio },
  };
}

/** The GPU adapter's vendor and description (WebGPU's `adapter.info`), or null. */
export async function gpuName(): Promise<string | null> {
  try {
    const gpu = (navigator as Partial<Pick<Navigator, 'gpu'>>).gpu;
    const adapter = gpu ? await gpu.requestAdapter() : null;
    if (!adapter) return null;
    const info = adapter.info;
    return [info.vendor, info.architecture, info.description].filter(Boolean).join(' ') || null;
  } catch {
    return null;
  }
}

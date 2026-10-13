// The capture (debug tooling, the debug overlay's "Copy capture" button): the frame on screen as a
// PNG data URL and what the game knows about the moment, as JSON on the clipboard, so a report of
// how something looks comes with where, when and in which build it was seen.
//
// What goes in is a registry of named sections: each system registers a function returning its part
// (the player, the LOD, the renderer, …), and the capture collects them all. A new system adds a
// section by registering it; the capture itself never changes.

/** A section's content: anything JSON can hold (bigints are written as decimal strings). */
export type CaptureSection = () => unknown;

export interface Capture {
  /** What this is, for whoever reads a pasted capture. */
  kind: 'dwell-capture';
  /** The capture's own format, raised when its shape changes. */
  format: 1;
  /** When it was taken (ISO 8601). */
  takenAt: string;
  /** Each registered section's content, by name; a section that failed holds `{ error }`. */
  sections: Record<string, unknown>;
  /** The frame as a `data:image/png;base64,…` URL, or null where it could not be read. */
  image: string | null;
}

/** Section names: camelCase letters and digits, so they read as JSON keys. */
const SECTION_NAME = /^[a-z][A-Za-z0-9]*$/;

export class CaptureRegistry {
  private readonly sections = new Map<string, CaptureSection>();

  /** Adds a section; a malformed or repeated name is an error (two systems claiming one key). */
  register(name: string, section: CaptureSection): void {
    if (!SECTION_NAME.test(name)) {
      throw new Error(`capture section "${name}": names are camelCase letters and digits`);
    }
    if (this.sections.has(name)) throw new Error(`capture section "${name}" is already registered`);
    this.sections.set(name, section);
  }

  /** The registered section names, in registration order. */
  names(): string[] {
    return [...this.sections.keys()];
  }

  /**
   * Collects every section. One that throws, or whose content JSON cannot hold, is recorded as
   * `{ error }` rather than losing the rest of the capture.
   */
  capture(image: string | null, now: Date = new Date()): Capture {
    const sections: Record<string, unknown> = {};
    for (const [name, section] of this.sections) {
      try {
        const value: unknown = section();
        JSON.stringify(value, jsonValue); // fails here, not when the capture is written
        sections[name] = value ?? null;
      } catch (err) {
        sections[name] = { error: err instanceof Error ? err.message : String(err) };
      }
    }
    return { kind: 'dwell-capture', format: 1, takenAt: now.toISOString(), sections, image };
  }
}

/** Bigints (seeds) as decimal strings: JSON has no 64-bit integers. */
function jsonValue(_key: string, value: unknown): unknown {
  return typeof value === 'bigint' ? value.toString() : value;
}

/** The capture as the text put on the clipboard: indented JSON, the image last. */
export function formatCapture(capture: Capture): string {
  return JSON.stringify(capture, jsonValue, 2);
}

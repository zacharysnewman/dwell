// The CPU profile in a Chrome trace (scripts/bench.ts records them with the V8 sampling profiler):
// self and inclusive time per function, per thread (the page's main thread, each worker), for
// docs/DEV_TOOLS.md's `<name>.profile.txt`. Pure: takes the trace's parsed JSON.

interface ProfileNode {
  id: number;
  parent?: number;
  callFrame: { functionName: string; url?: string };
}

interface TraceEvent {
  name?: string;
  ph?: string;
  pid?: number;
  tid?: number;
  id?: string;
  args?: {
    name?: string;
    data?: {
      cpuProfile?: { nodes?: ProfileNode[]; samples?: number[] };
      timeDeltas?: number[];
    };
  };
}

export interface FunctionTime {
  name: string;
  /** Samples in the function itself. */
  selfMs: number;
  /** Samples with the function anywhere on the stack (counted once per sample). */
  totalMs: number;
}

export interface ThreadProfile {
  thread: string;
  /** Time not idle. */
  totalMs: number;
  /** Busiest (self time) first. */
  functions: FunctionTime[];
}

/** Functions that are the profiler's bookkeeping rather than work. */
const NOT_WORK = new Set(['(idle)', '(program)', '(root)']);

function nodeName(n: ProfileNode): string {
  const f = n.callFrame;
  if (f.functionName) return f.functionName;
  return f.url ? `(anonymous ${f.url.split('/').pop() ?? ''})` : '(anonymous)';
}

/** Self and inclusive time per function and thread from a trace's `Profile`/`ProfileChunk` events. */
export function profileOf(trace: unknown): ThreadProfile[] {
  const events: TraceEvent[] = Array.isArray(trace)
    ? (trace as TraceEvent[])
    : ((trace as { traceEvents?: TraceEvent[] }).traceEvents ?? []);
  const names = new Map<string, string>();
  for (const e of events) {
    if (e.ph === 'M' && e.name === 'thread_name' && e.args?.name) {
      names.set(`${String(e.pid)}:${String(e.tid)}`, e.args.name);
    }
  }
  const threadName = (e: TraceEvent): string =>
    names.get(`${String(e.pid)}:${String(e.tid)}`) ?? `thread ${String(e.tid)}`;
  // A profile's thread is its `Profile` event's (its chunks come from the profiler's thread).
  const threadOf = new Map<string, string>();
  for (const e of events) {
    if (e.name === 'Profile') threadOf.set(`${String(e.pid)}:${e.id ?? ''}`, threadName(e));
  }
  // Per profile: its nodes, and the time sampled in each.
  const profiles = new Map<
    string,
    { thread: string; nodes: Map<number, ProfileNode>; self: Map<number, number> }
  >();
  for (const e of events) {
    if (e.name !== 'ProfileChunk' || !e.args?.data) continue;
    const key = `${String(e.pid)}:${e.id ?? ''}`;
    let p = profiles.get(key);
    if (!p) {
      p = { thread: threadOf.get(key) ?? threadName(e), nodes: new Map(), self: new Map() };
      profiles.set(key, p);
    }
    const { cpuProfile, timeDeltas = [] } = e.args.data;
    for (const n of cpuProfile?.nodes ?? []) p.nodes.set(n.id, n);
    const samples = cpuProfile?.samples ?? [];
    for (let i = 0; i < samples.length; i++) {
      // A sample lasts until the next one; the chunk's last uses its own delta.
      const us = i + 1 < timeDeltas.length ? timeDeltas[i + 1] : (timeDeltas.at(i) ?? 0);
      p.self.set(samples[i], (p.self.get(samples[i]) ?? 0) + Math.max(0, us));
    }
  }
  const byThread = new Map<string, Map<string, { self: number; total: number }>>();
  for (const p of profiles.values()) {
    const fns = byThread.get(p.thread) ?? new Map<string, { self: number; total: number }>();
    const at = (name: string): { self: number; total: number } => {
      let f = fns.get(name);
      if (!f) {
        f = { self: 0, total: 0 };
        fns.set(name, f);
      }
      return f;
    };
    for (const [id, us] of p.self) {
      const ms = us / 1000;
      const leaf = p.nodes.get(id);
      at(leaf ? nodeName(leaf) : `node ${String(id)}`).self += ms;
      // Inclusive: each function on the stack once (recursion counts once).
      const seen = new Set<string>();
      for (let n = leaf; n; n = n.parent === undefined ? undefined : p.nodes.get(n.parent)) {
        const name = nodeName(n);
        if (!seen.has(name)) {
          seen.add(name);
          at(name).total += ms;
        }
      }
    }
    byThread.set(p.thread, fns);
  }
  return [...byThread.entries()]
    .map(([thread, fns]) => {
      const functions = [...fns.entries()]
        .map(([name, t]) => ({ name, selfMs: t.self, totalMs: t.total }))
        .sort((a, b) => b.selfMs - a.selfMs);
      const totalMs = functions
        .filter((f) => !NOT_WORK.has(f.name))
        .reduce((a, f) => a + f.selfMs, 0);
      return { thread, totalMs, functions };
    })
    .sort((a, b) => b.totalMs - a.totalMs);
}

/** The functions that are work (not idle or the profiler's own). */
export function work(t: ThreadProfile): FunctionTime[] {
  return t.functions.filter((f) => !NOT_WORK.has(f.name));
}

/** The profile as text: each thread's busiest `top` functions by self time, then by inclusive. */
export function formatProfile(threads: ThreadProfile[], top = 40): string {
  const out: string[] = [];
  const row = (f: FunctionTime): string =>
    `  ${f.selfMs.toFixed(1).padStart(9)} ${f.totalMs.toFixed(1).padStart(9)}  ${f.name}`;
  for (const t of threads) {
    out.push(`${t.thread}: ${t.totalMs.toFixed(1)} ms busy`);
    out.push('    self ms  total ms  (by self)');
    out.push(...work(t).slice(0, top).map(row));
    out.push('    self ms  total ms  (by total)');
    out.push(
      ...[...work(t)]
        .sort((a, b) => b.totalMs - a.totalMs)
        .slice(0, top)
        .map(row),
    );
  }
  return `${out.join('\n')}\n`;
}

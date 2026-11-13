// A tiny HTTP-driven Chrome for recording (the `record` skill).
// node driver.mjs URL OUT_DIR [WIDTH HEIGHT] — POST /eval (body: JS, `d` = __dwell), /goto (body: URL),
// /rec/start?name=, /rec/stop, /shot?name=, /viewport?w=&h=, /errors, /quit. Listens on :4999 (or $RECORD_PORT).
// $RECORD_STORAGE: a JSON {localStorage key: value} set before every load (take.mjs passes a video's world.storage).
// /rec/start?name=&audio=1 also records the game's sound (__dwell.audioTap: effects and ambience, never music) and
// /rec/stop saves it beside the frames as OUT_DIR/<name>.webm with <name>.audio.json (its epoch start, the frames' clock).
import { createRequire } from 'node:module';
import http from 'node:http';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { audioStartExpr, audioStopExpr } from './audio.mjs';
const require = createRequire(new URL('../../../../client/package.json', import.meta.url));
const { chromium } = require('playwright');
const url = process.argv[2];
const OUT = process.argv[3] ?? 'record-out';
const W = +(process.argv[4] ?? 1080), H = +(process.argv[5] ?? 1920);
const browser = await chromium.launch({ headless: false, channel: 'chrome', args: [
  '--enable-unsafe-webgpu', '--disable-renderer-backgrounding', '--disable-backgrounding-occluded-windows',
  '--disable-background-timer-throttling', '--no-proxy-server',
  // the game's AudioContext starts without a click, so a take records its sound
  '--autoplay-policy=no-user-gesture-required'] });
const context = await browser.newContext({ viewport: { width: W, height: H } });
const page = await context.newPage();
const storage = { 'dwell.music': '"off"', ...JSON.parse(process.env.RECORD_STORAGE || '{}') };
await page.addInitScript((kv) => { for (const [k, v] of Object.entries(kv)) localStorage.setItem(k, v); }, storage);
let vw = W, vh = H;
await page.goto(url);
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
let cdp = await context.newCDPSession(page);
let rec = null;
cdp.on('Page.screencastFrame', async (f) => {
  cdp.send('Page.screencastFrameAck', { sessionId: f.sessionId }).catch(() => {});
  if (!rec) return;
  const i = rec.n++;
  writeFileSync(join(rec.dir, `f${String(i).padStart(5, '0')}.jpg`), Buffer.from(f.data, 'base64'));
  rec.times.push(f.metadata.timestamp);
});
const ev = (expr) => page.evaluate(`(async () => { const d = globalThis.__dwell; return (${expr}); })()`);
http.createServer(async (req, res) => {
  const u = new URL(req.url, 'http://x');
  let body = '';
  for await (const c of req) body += c;
  try {
    let out;
    if (u.pathname === '/goto') { await page.goto(body); out = 'ok'; }
    else if (u.pathname === '/eval') out = await ev(body);
    else if (u.pathname === '/rec/start') {
      const dir = join(OUT, u.searchParams.get('name')); mkdirSync(dir, { recursive: true });
      rec = { dir, name: u.searchParams.get('name'), n: 0, times: [], audio: null };
      if (u.searchParams.get('audio') === '1') rec.audio = await page.evaluate(audioStartExpr());
      await cdp.send('Page.startScreencast', { format: 'jpeg', quality: 92, maxWidth: vw, maxHeight: vh, everyNthFrame: 1 });
      out = 'recording ' + dir + (rec.audio ? ` with sound (${rec.audio.buses.join(', ')})` : '');
    } else if (u.pathname === '/rec/stop') {
      await cdp.send('Page.stopScreencast');
      const r = rec; rec = null;
      writeFileSync(join(r.dir, 'times.json'), JSON.stringify(r.times));
      const span = r.times.length > 1 ? r.times[r.times.length - 1] - r.times[0] : 0;
      out = { frames: r.n, seconds: span, fps: r.n / Math.max(span, 1e-6) };
      if (r.audio) {
        const a = await page.evaluate(audioStopExpr());
        if (a) {
          writeFileSync(join(OUT, r.name + '.webm'), Buffer.from(a.bytes, 'base64'));
          const { bytes: _, ...meta } = a;
          writeFileSync(join(OUT, r.name + '.audio.json'), JSON.stringify(meta));
          out.audio = { seconds: a.stopS - a.startS, buses: a.buses };
        }
      }
    } else if (u.pathname === '/viewport') {
      vw = +u.searchParams.get('w'); vh = +u.searchParams.get('h');
      await page.setViewportSize({ width: vw, height: vh }); out = `viewport ${vw}x${vh}`;
    } else if (u.pathname === '/shot') {
      await page.screenshot({ path: join(OUT, u.searchParams.get('name') + '.png') }); out = 'ok';
    } else if (u.pathname === '/errors') out = errors.slice(-20);
    else if (u.pathname === '/quit') { res.end('bye'); await browser.close(); process.exit(0); }
    res.end(JSON.stringify(out, null, 1));
  } catch (e) { res.statusCode = 500; res.end(String(e?.stack ?? e)); }
})
// take.mjs posts after long encodes: keep its idle socket longer than Node's 5 s, and let a long take's eval run
.on('listening', function () { this.keepAliveTimeout = 10 * 60_000; this.requestTimeout = 0; })
.listen(+(process.env.RECORD_PORT || 4999), () => console.log('driver on :' + (process.env.RECORD_PORT || 4999)));

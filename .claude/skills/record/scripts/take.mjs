// node take.mjs <path.json> <clipName> [warmS]
import { readFileSync } from 'node:fs';
const [pathFile, name, warmS = '10'] = process.argv.slice(2);
const path = JSON.parse(readFileSync(pathFile, 'utf8'));
const post = async (p, body = '') => (await fetch('http://localhost:4999' + p, { method: 'POST', body })).text();
const dur = path.keyframes[path.keyframes.length - 1].t;
const k0 = { ...path.keyframes[0], t: 0 };
const hold = { formatVersion: 1, keyframes: [k0, { t: 1000 }], segments: [{ move: 'cut' }] };
console.log(await post('/eval', `(async () => { d.theater('hud', { shown: false }); d.setView({ clouds: 'off' });
  d.theater('play', { path: ${JSON.stringify(hold)} }); await new Promise(r => setTimeout(r, ${+warmS * 1000})); return JSON.stringify(d.state().lod.drawn); })()`));
console.log(await post(`/rec/start?name=${name}`));
console.log(await post('/eval', `(async () => { d.theater('play', { path: ${JSON.stringify(path)} });
  while (!d.theater('state', {}).finished) await new Promise(r => setTimeout(r, 100)); await new Promise(r => setTimeout(r, 300)); return 'played'; })()`));
console.log(await post('/rec/stop'));

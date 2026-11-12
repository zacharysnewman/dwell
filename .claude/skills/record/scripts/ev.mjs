// node ev.mjs <file.js> [path]   or   node ev.mjs -e 'expr' [path]
import { readFileSync } from 'node:fs';
const a = process.argv[2];
const body = a === '-e' ? process.argv[3] : readFileSync(a, 'utf8');
const path = (a === '-e' ? process.argv[4] : process.argv[3]) ?? '/eval';
const r = await fetch('http://localhost:4999' + path, { method: 'POST', body });
console.log(await r.text());

// node go.mjs <script.js> '<argsJSON>' [recName]  — prepend args, post to the driver; with recName, setup→rec→run→stop
import { readFileSync } from 'node:fs';
const [file, args = '{}', rec] = process.argv.slice(2);
const src = readFileSync(file, 'utf8');
const post = async (p, body = '') => (await fetch('http://localhost:4999' + p, { method: 'POST', body })).text();
const a = JSON.parse(args);
if (rec) {
  console.log('setup', await post('/eval', `(globalThis.__args=${JSON.stringify({ ...a, phase: "setup" })}, ${src.replace(/^\s*\/\/.*$/gm, "")})`));
  console.log(await post(`/rec/start?name=${rec}`));
  console.log('run', await post('/eval', `(globalThis.__args=${JSON.stringify(a)}, ${src.replace(/^\s*\/\/.*$/gm, "")})`));
  console.log(await post('/rec/stop'));
} else {
  console.log(await post('/eval', `(globalThis.__args=${JSON.stringify(a)}, ${src.replace(/^\s*\/\/.*$/gm, "")})`));
}

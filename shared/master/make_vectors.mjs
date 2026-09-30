// Generates vectors.json: signed master requests (ARCHITECTURE.md §10.3) from a fixed Ed25519
// key, with Node's crypto (independent of the client's and the Worker's code). Both test suites
// check them: the client must produce these signatures, the master must accept them.
// Run: node shared/master/make_vectors.mjs
import { createHash, createPrivateKey, createPublicKey, sign } from 'node:crypto';
import { writeFileSync } from 'node:fs';

const seed = Buffer.alloc(32, 0).map((_, i) => i + 1); // 01 02 … 20
const pkcs8 = Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), seed]);
const privateKey = createPrivateKey({ key: pkcs8, format: 'der', type: 'pkcs8' });
const spki = createPublicKey(privateKey).export({ format: 'der', type: 'spki' });
const publicKey = spki.subarray(spki.length - 32);

function message(method, path, time, body) {
  const head = Buffer.from(`dwell-master-v1\n${method}\n${path}\n${String(time)}\n`, 'utf8');
  return Buffer.concat([head, createHash('sha256').update(body).digest()]);
}

const cases = [
  { method: 'POST', path: '/v1/whoami', time: 1790000000000, body: '' },
  { method: 'POST', path: '/v1/servers/register?x=1', time: 1790000012345, body: '{"port":4433}' },
  { method: 'DELETE', path: '/v1/rooms/KQ7-XM4', time: 1, body: 'é' },
].map((c) => ({
  ...c,
  message: message(c.method, c.path, c.time, Buffer.from(c.body, 'utf8')).toString('hex'),
  signature: sign(null, message(c.method, c.path, c.time, Buffer.from(c.body, 'utf8')), privateKey).toString('hex'),
}));

writeFileSync(
  new URL('./vectors.json', import.meta.url),
  JSON.stringify({ pkcs8: pkcs8.toString('hex'), publicKey: publicKey.toString('hex'), cases }, null, 2) + '\n',
);

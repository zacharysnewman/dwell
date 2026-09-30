// Tests run inside the Workers runtime (workerd via Miniflare), with the Durable Objects from
// wrangler.jsonc.
import { cloudflareTest } from '@cloudflare/vitest-pool-workers';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [cloudflareTest({ wrangler: { configPath: './wrangler.jsonc' } })],
  test: { include: ['test/**/*.test.ts'] },
});

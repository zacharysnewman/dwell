// Types for `env` and `exports` from cloudflare:workers in tests: the Worker's bindings and its
// main module.
import type { Env as MasterEnv } from '../src/env';

declare global {
  namespace Cloudflare {
    // eslint-disable-next-line @typescript-eslint/no-empty-object-type -- merges the bindings in
    interface Env extends MasterEnv {}
    interface GlobalProps {
      mainModule: typeof import('../src/index');
      durableNamespaces: 'Directory' | 'Room';
    }
  }
}

// The explored map's saves (exploredStore.ts): each merges a save's changed cells into the world's
// stored record here, off the page's frames. Saves run one after another, in the order sent.
import { mergeIntoStore, idbExplored, type ExploredAdd, type ExploredAdded } from './exploredStore';

interface WorkerScope {
  postMessage(message: ExploredAdded): void;
  onmessage: ((e: MessageEvent<ExploredAdd>) => void) | null;
}
const scope = self as unknown as WorkerScope;
const store = idbExplored();
let chain: Promise<void> = Promise.resolve();

scope.onmessage = (e: MessageEvent<ExploredAdd>) => {
  const { id, world, cells, heights } = e.data;
  chain = chain.then(() =>
    mergeIntoStore(store, world, cells, heights).then(
      () => {
        scope.postMessage({ id, ok: true } satisfies ExploredAdded);
      },
      () => {
        scope.postMessage({ id, ok: false } satisfies ExploredAdded);
      },
    ),
  );
};

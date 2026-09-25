// Device-key identity (ADR 0004, ARCHITECTURE.md §10.4). Each install has an Ed25519 key pair; the
// public key is the player id. The private key is a non-extractable WebCrypto key, so other pages on
// the shared dropkickarcade.com origin (ADR 0005) cannot copy it.

export interface DeviceKey {
  /** Raw 32-byte Ed25519 public key. */
  readonly publicKey: Uint8Array<ArrayBuffer>;
  sign(data: Uint8Array<ArrayBuffer>): Promise<Uint8Array<ArrayBuffer>>;
}

export interface KeyStore {
  load(): Promise<CryptoKeyPair | null>;
  save(pair: CryptoKeyPair): Promise<void>;
}

export class DeviceKeyUnavailableError extends Error {
  override name = 'DeviceKeyUnavailableError';
}

async function wrap(pair: CryptoKeyPair): Promise<DeviceKey> {
  const publicKey = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey));
  return {
    publicKey,
    async sign(data) {
      return new Uint8Array(await crypto.subtle.sign('Ed25519', pair.privateKey, data));
    },
  };
}

/** Loads this install's device key, generating and saving one on first run. */
export async function loadOrCreateDeviceKey(store: KeyStore): Promise<DeviceKey> {
  const existing = await store.load();
  if (existing) return wrap(existing);
  let pair: CryptoKeyPair;
  try {
    pair = await crypto.subtle.generateKey({ name: 'Ed25519' }, false, ['sign', 'verify']);
  } catch {
    throw new DeviceKeyUnavailableError('This browser does not support Ed25519 keys.');
  }
  await store.save(pair);
  return wrap(pair);
}

export class MemoryKeyStore implements KeyStore {
  private pair: CryptoKeyPair | null = null;
  load(): Promise<CryptoKeyPair | null> {
    return Promise.resolve(this.pair);
  }
  save(pair: CryptoKeyPair): Promise<void> {
    this.pair = pair;
    return Promise.resolve();
  }
}

/** Persists the key pair in a `dwell`-namespaced IndexedDB database (ADR 0005). */
export class IndexedDbKeyStore implements KeyStore {
  private static readonly DB = 'dwell';
  private static readonly STORE = 'identity';
  private static readonly KEY = 'device-key';

  private open(): Promise<IDBDatabase> {
    return new Promise((resolve, reject) => {
      const req = indexedDB.open(IndexedDbKeyStore.DB, 1);
      req.onupgradeneeded = () => {
        req.result.createObjectStore(IndexedDbKeyStore.STORE);
      };
      req.onsuccess = () => {
        resolve(req.result);
      };
      req.onerror = () => {
        reject(req.error ?? new Error('IndexedDB open failed'));
      };
    });
  }

  private async request<T>(
    mode: IDBTransactionMode,
    op: (s: IDBObjectStore) => IDBRequest,
  ): Promise<T> {
    const db = await this.open();
    try {
      return await new Promise<T>((resolve, reject) => {
        const req = op(
          db.transaction(IndexedDbKeyStore.STORE, mode).objectStore(IndexedDbKeyStore.STORE),
        );
        req.onsuccess = () => {
          resolve(req.result as T);
        };
        req.onerror = () => {
          reject(req.error ?? new Error('IndexedDB request failed'));
        };
      });
    } finally {
      db.close();
    }
  }

  async load(): Promise<CryptoKeyPair | null> {
    const pair = await this.request<CryptoKeyPair | undefined>('readonly', (s) =>
      s.get(IndexedDbKeyStore.KEY),
    );
    return pair ?? null;
  }

  async save(pair: CryptoKeyPair): Promise<void> {
    await this.request('readwrite', (s) => s.put(pair, IndexedDbKeyStore.KEY));
  }
}

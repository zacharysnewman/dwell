//! Which worker generates what (ARCHITECTURE.md §6.3): terrain work on the same columns goes to the
//! same worker. A generator caches the columns it works out per thread (the terrain volume's
//! reshaped columns and their slope's height field, `dwell-worldgen` `terrain.rs`), and the chunks
//! stacked over a column, and their neighbours, read the same columns: handed to one worker they
//! are worked out once. Measured natively on 4 threads, 867 chunks round seed 7's terraced
//! hillside: 1.4× one thread's speed dealt out in turn, 2.5× by chunk column, 2.9× by 2 × 2 blocks
//! of chunk columns. Both pools use it: the native server's generation threads
//! (`dwell-sim` `worldgen_pool.rs`) and the browser's generation workers (`dwell-client` `web/jobs.rs`).

/// Chunk columns (or LOD section columns) per side of a block that goes to one worker.
pub const BLOCK: i32 = 2;

/// The worker (of `workers`) that work on column (x, z) of level `level` belongs to: chunk
/// columns are level −1, LOD section columns their level. Spread evenly by a hash of the block.
pub fn worker_for(level: i32, x: i32, z: i32, workers: usize) -> usize {
    if workers <= 1 {
        return 0;
    }
    let (bx, bz) = (x.div_euclid(BLOCK), z.div_euclid(BLOCK));
    let mut h = (bx as u32).wrapping_mul(0x9E37_79B1)
        ^ (bz as u32).wrapping_mul(0x85EB_CA77)
        ^ (level as u32).wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h as usize) % workers
}

/// The worker a job goes to: its own (`preferred`, from [`worker_for`]) while that has fewer than
/// `limit` jobs, else the least busy (the first of them), so no worker waits while another's
/// queue is long.
pub fn pick_worker(busy: &[usize], preferred: Option<usize>, limit: usize) -> usize {
    if let Some(p) = preferred
        && busy.get(p).is_some_and(|&b| b < limit)
    {
        return p;
    }
    busy.iter()
        .enumerate()
        .min_by_key(|&(_, &b)| b)
        .map_or(0, |(i, _)| i)
}

/// The index in `queue` (in priority order) worker `worker` of `workers` takes next: the first
/// chunk of its own blocks, else the first of all (so it never idles while work is queued).
pub fn next_for(queue: &[crate::ChunkCoord], worker: usize, workers: usize) -> Option<usize> {
    queue
        .iter()
        .position(|c| worker_for(-1, c.x, c.z, workers) == worker)
        .or((!queue.is_empty()).then_some(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChunkCoord;

    #[test]
    fn a_block_of_columns_is_one_workers() {
        // face-a-only: columns have no up (x and z only).
        for workers in [2, 3, 4, 6] {
            for (bx, bz) in [(0, 0), (5, -3), (-7, 12)] {
                let w = worker_for(-1, bx * BLOCK, bz * BLOCK, workers);
                for dz in 0..BLOCK {
                    for dx in 0..BLOCK {
                        let (x, z) = (bx * BLOCK + dx, bz * BLOCK + dz);
                        assert_eq!(worker_for(-1, x, z, workers), w, "{x}, {z}");
                    }
                }
            }
            // And the blocks spread over every worker.
            let mut used = vec![0; workers];
            for bz in -10..10 {
                for bx in -10..10 {
                    used[worker_for(-1, bx * BLOCK, bz * BLOCK, workers)] += 1;
                }
            }
            assert!(used.iter().all(|&n| n > 400 / workers / 2), "{used:?}");
        }
    }

    #[test]
    fn a_job_goes_to_its_own_worker_unless_that_one_is_full() {
        // Red on the pools' old rule (always the least busy): worker 2's job went to worker 0.
        assert_eq!(pick_worker(&[0, 1, 1], Some(2), 2), 2);
        assert_eq!(pick_worker(&[0, 1, 2], Some(2), 2), 0);
        assert_eq!(pick_worker(&[1, 0, 2], None, 2), 1);
    }

    #[test]
    fn a_thread_takes_its_own_chunks_first_then_any() {
        // Red on the native pool's old rule (always the queue's front).
        let workers = 3;
        let mine = |w| move |c: &ChunkCoord| worker_for(-1, c.x, c.z, workers) == w;
        let queue: Vec<ChunkCoord> = (0..12).map(|i| ChunkCoord::new(i * 2, 0, 0)).collect();
        for w in 0..workers {
            let at = next_for(&queue, w, workers).expect("queued");
            if queue.iter().any(mine(w)) {
                assert!(mine(w)(&queue[at]), "worker {w} took {:?}", queue[at]);
                assert!(queue[..at].iter().all(|c| !mine(w)(c)), "its first");
            }
        }
        let others: Vec<ChunkCoord> = queue.iter().copied().filter(|c| !mine(0)(c)).collect();
        assert_eq!(next_for(&others, 0, workers), Some(0));
        assert_eq!(next_for(&[], 0, workers), None);
    }
}

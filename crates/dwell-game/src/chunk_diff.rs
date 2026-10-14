//! In-game "regenerate chunk and diff" (Phase 3e debug tooling): how many voxels of a loaded chunk
//! differ from the chunk as the generator makes it — the edits the world holds there.

use dwell_protocol::{CHUNK_SIZE, CHUNK_VOLUME};

/// Edge of the padded voxel block: the chunk plus one voxel on each side (the mesher's layout).
pub const PAD: usize = CHUNK_SIZE as usize + 2;
pub const PADDED_VOLUME: usize = PAD * PAD * PAD;

/// Index into padded voxels of chunk-local (x, y, z), each in −1..=CHUNK_SIZE.
pub fn padded_index(x: i32, y: i32, z: i32) -> usize {
    let p = PAD as i32;
    (x + 1 + p * (z + 1 + p * (y + 1))) as usize
}

/// Voxels where the chunk inside `padded` (the mesher's layout) differs from `generated`
/// (`x | y << 5 | z << 10`). Errors unless `padded` holds [`PADDED_VOLUME`] voxels and
/// `generated` [`CHUNK_VOLUME`].
pub fn count_changed(padded: &[u16], generated: &[u16]) -> Result<usize, &'static str> {
    if padded.len() != PADDED_VOLUME {
        return Err("padded voxels expected");
    }
    if generated.len() != CHUNK_VOLUME {
        return Err("a chunk's voxels expected");
    }
    let mut n = 0;
    for z in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let g = generated[(x | (y << 5) | (z << 10)) as usize];
                if padded[padded_index(x, y, z)] != g {
                    n += 1;
                }
            }
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_the_voxels_that_differ_from_generation_ignoring_the_apron() {
        let generated = vec![2u16; CHUNK_VOLUME];
        let mut padded = vec![0u16; PADDED_VOLUME];
        for z in 0..32 {
            for y in 0..32 {
                for x in 0..32 {
                    padded[padded_index(x, y, z)] = 2;
                }
            }
        }
        padded[padded_index(-1, 0, 0)] = 9; // apron: not this chunk
        assert_eq!(count_changed(&padded, &generated), Ok(0));
        padded[padded_index(3, 4, 5)] = 0;
        padded[padded_index(31, 31, 31)] = 16;
        assert_eq!(count_changed(&padded, &generated), Ok(2));
        assert!(count_changed(&generated, &generated).is_err());
    }
}

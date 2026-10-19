//! A microbench for registry sync's hot-path costs (ADR 0038): `info` (one comparison more than
//! indexing the generated array) and the session's per-chunk translation. Ignored by default:
//! `cargo test --release -p dwell-blocks --test perf -- --ignored --nocapture`.

use std::hint::black_box;
use std::time::Instant;

use dwell_blocks::sync::{Payload, map};
use dwell_blocks::{STATES, StateId, info};

#[test]
#[ignore]
fn info_and_translation_costs() {
    let n = 50_000_000usize;
    let ids: Vec<StateId> = (0..4096).map(|i| (i % STATES.len()) as StateId).collect();
    let t = Instant::now();
    let mut solid = 0usize;
    for i in 0..n {
        solid += usize::from(STATES[usize::from(ids[i & 4095])].solid);
    }
    let raw = t.elapsed().as_secs_f64() * 1e9 / n as f64;
    black_box(solid);
    let t = Instant::now();
    let mut solid = 0usize;
    for i in 0..n {
        solid += usize::from(info(ids[i & 4095]).solid);
    }
    let via_info = t.elapsed().as_secs_f64() * 1e9 / n as f64;
    black_box(solid);

    let identity = map(&Payload::served(), 0).expect("maps");
    let mut shifted = Payload::served();
    let last = shifted.blocks.len() - 1;
    shifted.blocks.swap(1, last);
    let mut next = 0u16;
    for b in &mut shifted.blocks {
        b.first_state = next;
        next += b.states.len() as u16;
    }
    let remap = map(&shifted, 0).expect("maps");
    assert!(identity.identity && !remap.identity);
    let chunk: Vec<u16> = (0..32 * 32 * 32).map(|i| (i % 3) as u16).collect();
    let time = |m: &dwell_blocks::sync::Mapping| {
        let reps = 2000;
        let t = Instant::now();
        for _ in 0..reps {
            let mut c = chunk.clone();
            assert!(m.to_client_ids(&mut c));
            black_box(&c);
        }
        t.elapsed().as_secs_f64() * 1e6 / f64::from(reps)
    };
    let (id_us, remap_us) = (time(&identity), time(&remap));
    println!(
        "info: {via_info:.2} ns/lookup (array index {raw:.2}); a 32^3 chunk's ids: identity \
         {id_us:.1} us, translated {remap_us:.1} us (both with the copy)"
    );
}

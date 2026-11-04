//! The world map's rivers as water needs them (ADR 0059): where a column lies on one, and the
//! pool graph — the generated pool a column lies in, what comes into a pool at rest, its lip.

use dwell_world::Face;
pub use dwell_worldgen::terrain::{GraphLip, GraphPool, PoolInflows, PoolKey, RiverNext};

/// The world map's rivers, as water needs them: where a column lies on one, how long each is and
/// where it goes (the server's: `dwell_worldgen`'s terrain; a test's: its own).
pub trait Rivers: Send + Sync {
    /// The river at face-local column (x, z) and how far along it (m from its source).
    fn at(&self, face: Face, x: i32, z: i32) -> Option<(u32, f64)>;
    /// Its length (m).
    fn length(&self, face: Face, river: u32) -> f64;
    /// Where it goes at its end.
    fn next(&self, face: Face, river: u32) -> RiverNext;
    /// The pool of the world map's pool graph full-detail column (x, z) lies in (ADR 0059).
    fn pool(&self, _face: Face, _x: i32, _z: i32) -> Option<PoolKey> {
        None
    }
    /// What comes into pool `key` at rest, by the graph.
    fn inflows(&self, _face: Face, _key: PoolKey) -> PoolInflows {
        PoolInflows::default()
    }
    /// Pool `key`'s lip out, by the graph.
    fn lip(&self, _face: Face, _key: PoolKey) -> Option<GraphLip> {
        None
    }
    /// Pool `key` (its reach and surface estimate), by the graph.
    fn graph_pool(&self, _face: Face, _key: PoolKey) -> Option<GraphPool> {
        None
    }
}

impl Rivers for dwell_worldgen::BifacialTerrain {
    fn at(&self, face: Face, x: i32, z: i32) -> Option<(u32, f64)> {
        self.local(face).river_at(x, z)
    }
    fn length(&self, face: Face, river: u32) -> f64 {
        self.local(face).river_length(river)
    }
    fn next(&self, face: Face, river: u32) -> RiverNext {
        self.local(face).river_next(river)
    }
    fn pool(&self, face: Face, x: i32, z: i32) -> Option<PoolKey> {
        self.local(face).pool_at(i64::from(x), i64::from(z))
    }
    fn inflows(&self, face: Face, key: PoolKey) -> PoolInflows {
        self.local(face).pool_inflows(key)
    }
    fn lip(&self, face: Face, key: PoolKey) -> Option<GraphLip> {
        self.local(face).pool_lip(key)
    }
    fn graph_pool(&self, face: Face, key: PoolKey) -> Option<GraphPool> {
        self.local(face).graph_pool(key)
    }
}

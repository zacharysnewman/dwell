//! The process's block table (ADR 0038): the build's generated states (`STATES`, ids
//! `0..STATES.len()`) followed by states registered at run time — a server's own blocks (mods, the
//! test fixture), placeholders for states a world file names that the server lacks, and on a
//! client the **synced** states a server sent that the build does not have. The table only grows;
//! a registered state keeps its id and its `&'static` row for the life of the process, and a
//! registration identical to an earlier one (same origin, names and definitions) returns the
//! earlier ids, so reconnecting does not grow it.
//!
//! Reads are lock-free: build ids index the generated array (one comparison more than before),
//! registered ids a segmented append-only store.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::{BLOCKS, PROPERTIES, PROPERTY_VALUES, STATES, StateId, StateInfo};

/// Where a block in the table came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    /// The build's data (`shared/blocks/*.json`).
    Build,
    /// Registered by a server from data at run time (mods; tests' fixture blocks).
    Server,
    /// A server's stand-in for a state its world file names and it does not have: kept under that
    /// exact string so saving writes it back.
    Placeholder,
    /// Received from a server at join (registry sync), unknown to the build.
    Synced,
}

impl Origin {
    /// Part of a server's own registry (what it sends at join).
    pub fn served(self) -> bool {
        self != Origin::Synced
    }
}

/// A block property and its values, in declared order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Property {
    pub name: &'static str,
    pub values: &'static [&'static str],
}

/// A block: its namespaced id, the run of state ids it owns and its properties (alphabetical).
#[derive(Clone, Copy, Debug)]
pub struct Block {
    pub name: &'static str,
    pub first_state: StateId,
    pub state_count: u16,
    pub properties: &'static [Property],
    pub origin: Origin,
}

impl Block {
    /// The state ids it owns.
    pub fn states(&self) -> std::ops::Range<usize> {
        let first = usize::from(self.first_state);
        first..first + usize::from(self.state_count)
    }
}

/// A block to register: its states' rows in property order (keys alphabetical, the last key
/// varying fastest, values in declared order), their names canonical. `StateInfo::block` is set
/// by the registration.
#[derive(Clone, Debug)]
pub struct NewBlock {
    pub name: String,
    pub properties: Vec<(String, Vec<String>)>,
    pub states: Vec<StateInfo>,
}

const SEG: usize = 256;
const SEGS: usize = 1 << 16 >> 8;

struct Store<T: 'static> {
    segs: [OnceLock<Box<[OnceLock<&'static T>]>>; SEGS],
}

impl<T: Sync + Send> Store<T> {
    const fn new() -> Self {
        Self {
            segs: [const { OnceLock::new() }; SEGS],
        }
    }
    fn get(&self, i: usize) -> Option<&'static T> {
        self.segs.get(i / SEG)?.get()?.get(i % SEG)?.get().copied()
    }
    fn put(&self, i: usize, v: &'static T) {
        let seg = self.segs[i / SEG].get_or_init(|| (0..SEG).map(|_| OnceLock::new()).collect());
        let _ = seg[i % SEG].set(v);
    }
}

static EXTRA_STATES: Store<StateInfo> = Store::new();
static EXTRA_BLOCKS: Store<Block> = Store::new();
/// Registered states and blocks, published after their rows are stored.
static STATE_LEN: AtomicUsize = AtomicUsize::new(0);
static BLOCK_LEN: AtomicUsize = AtomicUsize::new(0);
/// Serialises registrations.
static WRITER: Mutex<()> = Mutex::new(());

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_owned().into_boxed_str())
}

/// The build's blocks as [`Block`]s (from the generated `BLOCKS`/`PROPERTIES`).
fn build_blocks() -> &'static [Block] {
    static B: OnceLock<Vec<Block>> = OnceLock::new();
    B.get_or_init(|| {
        BLOCKS
            .iter()
            .map(|b| {
                let props: Vec<Property> = (0..b.property_count)
                    .map(|k| {
                        let p = &PROPERTIES[b.first_property + k];
                        Property {
                            name: p.name,
                            values: &PROPERTY_VALUES[p.first_value..p.first_value + p.value_count],
                        }
                    })
                    .collect();
                Block {
                    name: b.name,
                    first_state: b.first_state,
                    state_count: b.state_count,
                    properties: Box::leak(props.into_boxed_slice()),
                    origin: Origin::Build,
                }
            })
            .collect()
    })
}

/// States in the table: the build's and every registered one.
pub fn state_count() -> usize {
    STATES.len() + STATE_LEN.load(Ordering::Acquire)
}

/// Blocks in the table.
pub fn block_count() -> usize {
    BLOCKS.len() + BLOCK_LEN.load(Ordering::Acquire)
}

/// The state's row; `None` beyond the table.
#[inline]
pub fn try_info(id: StateId) -> Option<&'static StateInfo> {
    let i = usize::from(id);
    match STATES.get(i) {
        Some(s) => Some(s),
        None => EXTRA_STATES.get(i - STATES.len()),
    }
}

/// The state's row. Panics beyond the table (ids come from the table or through a session's
/// checked mapping).
#[inline]
pub fn info(id: StateId) -> &'static StateInfo {
    try_info(id).unwrap_or_else(|| panic!("state id {id} is not in the block table"))
}

/// Block `i` of the table.
pub fn block(i: usize) -> Option<&'static Block> {
    match build_blocks().get(i) {
        Some(b) => Some(b),
        None => EXTRA_BLOCKS.get(i - BLOCKS.len()),
    }
}

/// The block a state belongs to.
pub fn block_of(id: StateId) -> &'static Block {
    block(info(id).block).expect("every state's block is in the table")
}

/// Every block in the table, in id order.
pub fn blocks() -> impl Iterator<Item = &'static Block> {
    (0..block_count()).filter_map(block)
}

/// Whether `id` is one of the build's own states.
pub fn is_build_state(id: StateId) -> bool {
    usize::from(id) < STATES.len()
}

/// Registers a block (see the module docs): its first state id. An identical earlier registration
/// of the same origin returns its ids. Fails if the states do not match the properties, a name is
/// not canonical, or the table would pass `u16`.
pub fn register(origin: Origin, block: NewBlock) -> Result<StateId, String> {
    let expected: usize = block.properties.iter().map(|(_, v)| v.len()).product();
    if block.states.is_empty() || block.states.len() != expected {
        return Err(format!(
            "{}: {} states for properties giving {expected}",
            block.name,
            block.states.len()
        ));
    }
    for (k, s) in block.states.iter().enumerate() {
        let want = canonical(&block.name, &block.properties, k);
        if s.name != want {
            return Err(format!(
                "{}: state {k} is named `{}`, not `{want}`",
                block.name, s.name
            ));
        }
    }
    let _guard = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    // An identical registration already in the table.
    for b in blocks().filter(|b| b.origin == origin && b.name == block.name) {
        let same_props = b.properties.len() == block.properties.len()
            && b.properties
                .iter()
                .zip(&block.properties)
                .all(|(p, (n, v))| {
                    p.name == n
                        && p.values.len() == v.len()
                        && p.values.iter().zip(v).all(|(a, b)| a == b)
                });
        if same_props
            && b.states().zip(&block.states).all(|(id, s)| {
                let have = info(id as StateId);
                same_definition(have, s)
            })
        {
            return Ok(b.first_state);
        }
    }
    let first = state_count();
    if first + block.states.len() > usize::from(StateId::MAX) + 1 {
        return Err(format!("{}: the block table is full", block.name));
    }
    let block_index = block_count();
    let properties: Vec<Property> = block
        .properties
        .iter()
        .map(|(n, v)| Property {
            name: leak(n),
            values: Box::leak(
                v.iter()
                    .map(|s| leak(s))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
        })
        .collect();
    for (k, s) in block.states.iter().enumerate() {
        let mut row = *s;
        row.block = block_index;
        EXTRA_STATES.put(first + k - STATES.len(), Box::leak(Box::new(row)));
    }
    EXTRA_BLOCKS.put(
        block_index - BLOCKS.len(),
        Box::leak(Box::new(Block {
            name: leak(&block.name),
            first_state: first as StateId,
            state_count: block.states.len() as u16,
            properties: Box::leak(properties.into_boxed_slice()),
            origin,
        })),
    );
    STATE_LEN.fetch_add(block.states.len(), Ordering::AcqRel);
    BLOCK_LEN.fetch_add(1, Ordering::AcqRel);
    Ok(first as StateId)
}

/// Two rows describe the same state (every field but the block index).
pub fn same_definition(a: &StateInfo, b: &StateInfo) -> bool {
    let mut b = *b;
    b.block = a.block;
    format!("{a:?}") == format!("{b:?}")
}

/// The canonical string of state `k` (in property order) of a block.
pub fn canonical(name: &str, properties: &[(String, Vec<String>)], k: usize) -> String {
    if properties.is_empty() {
        return name.to_owned();
    }
    let mut idx = vec![0; properties.len()];
    let mut rest = k;
    for (i, (_, v)) in properties.iter().enumerate().rev() {
        idx[i] = rest % v.len().max(1);
        rest /= v.len().max(1);
    }
    let list: Vec<String> = properties
        .iter()
        .zip(&idx)
        .map(|((n, v), &i)| format!("{n}={}", v[i]))
        .collect();
    format!("{name}[{}]", list.join(","))
}

/// A leaked copy of `s` (for rows of registered states, whose strings live as long as the table).
pub fn intern(s: &str) -> &'static str {
    leak(s)
}

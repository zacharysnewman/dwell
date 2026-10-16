//! The string layer over the table (docs/BLOCK_REGISTRY.md): canonical state strings, parsing
//! with defaults, and reading or changing one property of a state. Works on the build's states and
//! on registered ones alike (`crate::table`).

use crate::{Block, StateId, block_of, blocks, info, try_info};

/// The value index of each of a block's properties in a state (mixed radix, last varies fastest).
fn indices(block: &Block, state: StateId) -> Vec<usize> {
    let mut out = vec![0; block.properties.len()];
    let mut rest = usize::from(state - block.first_state);
    for k in (0..block.properties.len()).rev() {
        let n = block.properties[k].values.len();
        out[k] = rest % n;
        rest /= n;
    }
    out
}

fn state_of(block: &Block, indices: &[usize]) -> StateId {
    let mut id = 0usize;
    for (k, &i) in indices.iter().enumerate() {
        id = id * block.properties[k].values.len() + i;
    }
    block.first_state + id as StateId
}

/// The block with a namespaced id: the build's, else the first registered one.
pub fn find_block(id: &str) -> Option<&'static Block> {
    blocks().find(|b| b.name == id)
}

/// Parses `ns:name` or `ns:name[k=v,…]`: any key order, missing properties take their defaults
/// (the first declared value). An error names the unknown block, property or value, a repeated
/// key, or malformed text.
pub fn parse_state(text: &str) -> Result<StateId, String> {
    let (id, list) = match text.find('[') {
        Some(open) => {
            if !text.ends_with(']') {
                return Err(format!("missing ']' in \"{text}\""));
            }
            (&text[..open], Some(&text[open + 1..text.len() - 1]))
        }
        None => (text, None),
    };
    let block = find_block(id).ok_or_else(|| format!("unknown block \"{id}\""))?;
    let n = block.properties.len();
    let mut idx = vec![0usize; n];
    let mut given = vec![false; n];
    if let Some(list) = list.filter(|l| !l.is_empty()) {
        if list.ends_with(',') {
            return Err(format!("trailing ',' in \"{text}\""));
        }
        for pair in list.split(',') {
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| format!("expected key=value: \"{pair}\""))?;
            let k = (0..n)
                .find(|&k| block.properties[k].name == key)
                .ok_or_else(|| format!("{id} has no property \"{key}\""))?;
            if given[k] {
                return Err(format!("repeated property \"{key}\""));
            }
            given[k] = true;
            idx[k] = block.properties[k]
                .values
                .iter()
                .position(|v| *v == value)
                .ok_or_else(|| format!("{id}.{key} has no value \"{value}\""))?;
        }
    }
    Ok(state_of(block, &idx))
}

/// The value of a state's property, if its block has it.
pub fn state_property(state: StateId, name: &str) -> Option<&'static str> {
    try_info(state)?;
    let block = block_of(state);
    let idx = indices(block, state);
    (0..block.properties.len())
        .find(|&k| block.properties[k].name == name)
        .map(|k| block.properties[k].values[idx[k]])
}

/// The same block with `name` set to `value`; `None` if the block has no such property or value.
pub fn with_property(state: StateId, name: &str, value: &str) -> Option<StateId> {
    try_info(state)?;
    let block = block_of(state);
    let mut idx = indices(block, state);
    let k = (0..block.properties.len()).find(|&k| block.properties[k].name == name)?;
    idx[k] = block.properties[k].values.iter().position(|v| *v == value)?;
    Some(state_of(block, &idx))
}

/// The same block facing `facing` (its `facingProperty`, BLOCK_REGISTRY.md), every other property
/// kept; `state` itself if the block has no facing or no state facing that way.
pub fn with_facing(state: StateId, facing: crate::Facing) -> StateId {
    if try_info(state).is_none_or(|s| s.facing == facing) {
        return state;
    }
    let block = block_of(state);
    let have = indices(block, state);
    (block.first_state..block.first_state + block.state_count)
        .find(|&s| {
            info(s).facing == facing
                && indices(block, s)
                    .iter()
                    .zip(&have)
                    .filter(|(a, b)| a != b)
                    .count()
                    == 1
        })
        .unwrap_or(state)
}

/// The state a block takes turned upside down (vertical mirror): `half` swapped; every other state
/// is its own mirror. Face B's chunks are face-local ones flipped with this.
pub fn mirror(state: StateId) -> StateId {
    match state_property(state, "half") {
        Some(half) => with_property(
            state,
            "half",
            if half == "bottom" { "top" } else { "bottom" },
        )
        .unwrap_or(state),
        None => state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::STATES;

    #[test]
    fn parses_with_defaults_and_rejects_bad_text() {
        let east = parse_state("dwell:ladder[facing=east]").expect("parses");
        assert_eq!(
            STATES[usize::from(east)].name,
            "dwell:ladder[facing=east,flooded=false]"
        );
        assert_eq!(
            parse_state("dwell:ladder[flooded=true,facing=east]"),
            with_property(east, "flooded", "true").ok_or(String::new())
        );
        assert!(parse_state("dwell:nope").is_err());
        assert!(parse_state("dwell:ladder[facing=up]").is_err());
        assert!(parse_state("dwell:ladder[facing=east,facing=west]").is_err());
        assert!(parse_state("dwell:ladder[facing=east,]").is_err());
        assert!(parse_state("dwell:ladder[facing=east").is_err());
        for (i, s) in STATES.iter().enumerate() {
            assert_eq!(parse_state(s.name), Ok(i as StateId));
        }
    }

    #[test]
    fn properties_read_and_change() {
        let l = parse_state("dwell:ladder").expect("ladder");
        assert_eq!(state_property(l, "facing"), Some("north"));
        assert_eq!(state_property(l, "half"), None);
        assert_eq!(with_property(l, "facing", "sideways"), None);
        assert_eq!(mirror(l), l);
    }

    #[test]
    fn turns_a_facing_keeping_the_other_properties() {
        use crate::Facing;
        let l = parse_state("dwell:ladder[facing=north,flooded=true]").expect("ladder");
        assert_eq!(
            STATES[usize::from(with_facing(l, Facing::West))].name,
            "dwell:ladder[facing=west,flooded=true]"
        );
        assert_eq!(with_facing(l, Facing::North), l);
        let stone = parse_state("dwell:stone").expect("stone");
        assert_eq!(with_facing(stone, Facing::East), stone);
    }
}

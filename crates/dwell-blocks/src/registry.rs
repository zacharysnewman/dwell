//! The string layer over the tables (docs/BLOCK_REGISTRY.md): canonical state strings, parsing
//! with defaults, and reading or changing one property of a state.

use crate::{BLOCKS, BlockDef, PROPERTIES, PROPERTY_VALUES, STATES, StateId, block_of};

fn property(block: &BlockDef, k: usize) -> &'static crate::PropertyDef {
    &PROPERTIES[block.first_property + k]
}

fn values(p: &crate::PropertyDef) -> &'static [&'static str] {
    &PROPERTY_VALUES[p.first_value..p.first_value + p.value_count]
}

/// The value index of each of a block's properties in a state (mixed radix, last varies fastest).
fn indices(block: &BlockDef, state: StateId) -> Vec<usize> {
    let mut out = vec![0; block.property_count];
    let mut rest = usize::from(state - block.first_state);
    for k in (0..block.property_count).rev() {
        let n = property(block, k).value_count;
        out[k] = rest % n;
        rest /= n;
    }
    out
}

fn state_of(block: &BlockDef, indices: &[usize]) -> StateId {
    let mut id = 0usize;
    for (k, &i) in indices.iter().enumerate() {
        id = id * property(block, k).value_count + i;
    }
    block.first_state + id as StateId
}

/// The block with a namespaced id.
pub fn find_block(id: &str) -> Option<&'static BlockDef> {
    BLOCKS.iter().find(|b| b.name == id)
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
    let mut idx = vec![0usize; block.property_count];
    let mut given = vec![false; block.property_count];
    if let Some(list) = list.filter(|l| !l.is_empty()) {
        if list.ends_with(',') {
            return Err(format!("trailing ',' in \"{text}\""));
        }
        for pair in list.split(',') {
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| format!("expected key=value: \"{pair}\""))?;
            let k = (0..block.property_count)
                .find(|&k| property(block, k).name == key)
                .ok_or_else(|| format!("{id} has no property \"{key}\""))?;
            if given[k] {
                return Err(format!("repeated property \"{key}\""));
            }
            given[k] = true;
            idx[k] = values(property(block, k))
                .iter()
                .position(|v| *v == value)
                .ok_or_else(|| format!("{id}.{key} has no value \"{value}\""))?;
        }
    }
    Ok(state_of(block, &idx))
}

/// The value of a state's property, if its block has it.
pub fn state_property(state: StateId, name: &str) -> Option<&'static str> {
    if usize::from(state) >= STATES.len() {
        return None;
    }
    let block = block_of(state);
    let idx = indices(block, state);
    (0..block.property_count)
        .find(|&k| property(block, k).name == name)
        .map(|k| values(property(block, k))[idx[k]])
}

/// The same block with `name` set to `value`; `None` if the block has no such property or value.
pub fn with_property(state: StateId, name: &str, value: &str) -> Option<StateId> {
    if usize::from(state) >= STATES.len() {
        return None;
    }
    let block = block_of(state);
    let mut idx = indices(block, state);
    let k = (0..block.property_count).find(|&k| property(block, k).name == name)?;
    idx[k] = values(property(block, k))
        .iter()
        .position(|v| *v == value)?;
    Some(state_of(block, &idx))
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
}

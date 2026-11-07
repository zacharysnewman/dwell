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
/// (the first declared value). A property or value this process doesn't know makes the state an
/// **alias** (ADR 0063, [`crate::register_alias`]): a state under the exact string, behaving as its
/// known part (unknown keys dropped, unknown values at the property's default), so a newer build's
/// or a mod's states load, play as their base and save back unchanged. An error names an unknown
/// block, a repeated key or malformed text.
pub fn parse_state(text: &str) -> Result<StateId, String> {
    let (base, unknown) = parse(text)?;
    if unknown.is_empty() {
        return Ok(base);
    }
    crate::register_alias(text, base)
}

/// Like [`parse_state`], but a property or value this process doesn't know is an error rather
/// than an alias: for the build's own content (data files, tools), where it is a typo.
pub fn parse_known_state(text: &str) -> Result<StateId, String> {
    let (base, unknown) = parse(text)?;
    match unknown.into_iter().next() {
        None => Ok(base),
        Some(e) => Err(e),
    }
}

/// The state the known part of `text` names, and the unknown properties and values in it (as
/// error messages).
fn parse(text: &str) -> Result<(StateId, Vec<String>), String> {
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
    let mut unknown = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    if let Some(list) = list.filter(|l| !l.is_empty()) {
        if list.ends_with(',') {
            return Err(format!("trailing ',' in \"{text}\""));
        }
        for pair in list.split(',') {
            let (key, value) = pair
                .split_once('=')
                .filter(|(k, v)| !k.is_empty() && !v.is_empty())
                .ok_or_else(|| format!("expected key=value: \"{pair}\""))?;
            if seen.contains(&key) {
                return Err(format!("repeated property \"{key}\""));
            }
            seen.push(key);
            let Some(k) = (0..n).find(|&k| block.properties[k].name == key) else {
                unknown.push(format!("{id} has no property \"{key}\""));
                continue;
            };
            match block.properties[k].values.iter().position(|v| *v == value) {
                Some(i) => idx[k] = i,
                None => unknown.push(format!("{id}.{key} has no value \"{value}\"")),
            }
        }
    }
    Ok((state_of(block, &idx), unknown))
}

/// The value of a state's property, if its block has it.
pub fn state_property(state: StateId, name: &str) -> Option<&'static str> {
    let state = crate::base_state(state);
    try_info(state)?;
    let block = block_of(state);
    let idx = indices(block, state);
    (0..block.properties.len())
        .find(|&k| block.properties[k].name == name)
        .map(|k| block.properties[k].values[idx[k]])
}

/// The same block with `name` set to `value`; `None` if the block has no such property or value.
/// An alias (ADR 0063) is changed as its base: the result is the base's state, the unknown part
/// dropped (a rule rewriting the block replaces it).
pub fn with_property(state: StateId, name: &str, value: &str) -> Option<StateId> {
    let state = crate::base_state(state);
    try_info(state)?;
    let block = block_of(state);
    let mut idx = indices(block, state);
    let k = (0..block.properties.len()).find(|&k| block.properties[k].name == name)?;
    idx[k] = block.properties[k]
        .values
        .iter()
        .position(|v| *v == value)?;
    Some(state_of(block, &idx))
}

/// The same block facing `facing` (its `facingProperty`, BLOCK_REGISTRY.md), every other property
/// kept; `state` itself if the block has no facing or no state facing that way.
pub fn with_facing(state: StateId, facing: crate::Facing) -> StateId {
    if try_info(state).is_none_or(|s| s.facing == facing) {
        return state;
    }
    let state = crate::base_state(state);
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
        assert!(parse_known_state("dwell:ladder[facing=up]").is_err());
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

    #[test]
    fn an_unknown_property_or_value_is_an_alias_of_the_known_part() {
        // ADR 0063: a newer build's or a mod's property stays on the block.
        let text = "dwell:leaves[mymod:wet=7,flooded=true]";
        let alias = parse_state(text).expect("an alias");
        let base = parse_state("dwell:leaves[flooded=true]").expect("leaves");
        assert_ne!(alias, base);
        assert!(!crate::is_build_state(alias));
        assert_eq!(crate::alias_base(alias), Some(base));
        assert_eq!(crate::info(alias).name, text, "kept under the exact string");
        let mut row = *crate::info(alias);
        row.name = crate::info(base).name;
        assert!(
            crate::same_definition(&row, crate::info(base)),
            "behaves as its base"
        );
        assert_eq!(crate::block_of(alias).name, "dwell:leaves");
        assert_eq!(state_property(alias, "flooded"), Some("true"));
        assert_eq!(parse_state(text), Ok(alias), "one string, one id");
        // an unknown value: the property's default
        let up = parse_state("dwell:ladder[facing=up]").expect("an alias");
        assert_eq!(
            crate::alias_base(up),
            parse_state("dwell:ladder[facing=north]").ok()
        );
        // the strict parse refuses both; an unknown block is still an error
        assert!(parse_known_state(text).is_err());
        assert!(parse_state("mymod:moss[wet=7]").is_err());
        assert!(parse_state("dwell:leaves[flooded=true,flooded=false]").is_err());
        assert!(crate::state(text).is_none(), "built-in lookups stay strict");
    }
}

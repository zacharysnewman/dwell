//! Items held by villagers, carts and storehouses, and the one way they move between them (B5,
//! docs/BLUEPRINTS.md §9). Every holder — a villager's carried [`Load`] (at most its job's
//! `carryMax`), a storehouse's [`Stock`], and later a cart's — is a [`Holder`]; items go from one
//! to another only through [`transfer`], so the later jobs (foresters felling, carters hauling to
//! the storehouse) reuse it: a forester's load goes to a cart, a cart's to a storehouse, a
//! storehouse's to a builder.

use dwell_blocks::StateId;

/// Something that holds items: it can say how many of a block it could give or take, and give
/// or take them.
pub trait Holder {
    /// How many of `state` it could give now.
    fn available(&self, state: StateId) -> u32;
    /// How many of `state` it could take now.
    fn room(&self, state: StateId) -> u32;
    /// Removes up to `n` of `state`; how many were removed.
    fn give(&mut self, state: StateId, n: u32) -> u32;
    /// Adds up to `n` of `state`; how many were added.
    fn take(&mut self, state: StateId, n: u32) -> u32;
}

/// Moves up to `n` of `state` from `from` to `to`, as many as one has and the other has room for;
/// how many moved.
pub fn transfer(from: &mut dyn Holder, to: &mut dyn Holder, state: StateId, n: u32) -> u32 {
    let n = n.min(from.available(state)).min(to.room(state));
    if n == 0 {
        return 0;
    }
    let given = from.give(state, n);
    let taken = to.take(state, given);
    debug_assert_eq!(given, taken, "room was checked");
    taken
}

/// Moves everything `from` holds of the blocks in `wanted` (each up to its count, in order) into
/// `to`; how many moved in all.
pub fn transfer_list(from: &mut dyn Holder, to: &mut dyn Holder, wanted: &[(StateId, u32)]) -> u32 {
    wanted
        .iter()
        .map(|&(state, n)| transfer(from, to, state, n))
        .sum()
}

/// A carried load: blocks and their counts in the order taken, at most `cap` items in all.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Load {
    pub items: Vec<(StateId, u32)>,
    pub cap: u32,
}

impl Load {
    pub fn new(cap: u32) -> Self {
        Self {
            items: Vec::new(),
            cap,
        }
    }

    /// How many items it holds in all.
    pub fn count(&self) -> u32 {
        self.items.iter().map(|i| i.1).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// Whether it holds one of `state`.
    pub fn has(&self, state: StateId) -> bool {
        self.available(state) > 0
    }

    /// Each piece's block in load order (the stack drawn on a back).
    pub fn pieces(&self) -> Vec<StateId> {
        self.items
            .iter()
            .flat_map(|&(s, n)| std::iter::repeat_n(s, n as usize))
            .collect()
    }

    /// Everything it holds, emptied into `to` (what didn't fit stays); how many moved.
    pub fn empty_into(&mut self, to: &mut dyn Holder) -> u32 {
        let all = self.items.clone();
        transfer_list(self, to, &all)
    }
}

impl Holder for Load {
    fn available(&self, state: StateId) -> u32 {
        self.items.iter().find(|i| i.0 == state).map_or(0, |i| i.1)
    }

    fn room(&self, _: StateId) -> u32 {
        self.cap.saturating_sub(self.count())
    }

    fn give(&mut self, state: StateId, n: u32) -> u32 {
        let Some(i) = self.items.iter().position(|i| i.0 == state) else {
            return 0;
        };
        let n = n.min(self.items[i].1);
        self.items[i].1 -= n;
        if self.items[i].1 == 0 {
            self.items.remove(i);
        }
        n
    }

    fn take(&mut self, state: StateId, n: u32) -> u32 {
        let n = n.min(self.room(state));
        if n == 0 {
            return 0;
        }
        match self.items.iter_mut().find(|i| i.0 == state) {
            Some(i) => i.1 += n,
            None => self.items.push((state, n)),
        }
        n
    }
}

/// A storehouse's stock (`storehouse.stock`): unlimited for now (it gives anything, keeps
/// anything), or counted (what has been put in, B5's later phases: deposits from harvesting).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stock {
    Unlimited,
    Counted(Vec<(StateId, u32)>),
}

impl Stock {
    /// A new storehouse's stock in the data's mode.
    pub fn of(mode: crate::blueprint::StockMode) -> Self {
        match mode {
            crate::blueprint::StockMode::Unlimited => Self::Unlimited,
        }
    }
}

impl Holder for Stock {
    fn available(&self, state: StateId) -> u32 {
        match self {
            Self::Unlimited => u32::MAX,
            Self::Counted(items) => items.iter().find(|i| i.0 == state).map_or(0, |i| i.1),
        }
    }

    fn room(&self, _: StateId) -> u32 {
        u32::MAX
    }

    fn give(&mut self, state: StateId, n: u32) -> u32 {
        match self {
            Self::Unlimited => n,
            Self::Counted(items) => {
                let Some(i) = items.iter().position(|i| i.0 == state) else {
                    return 0;
                };
                let n = n.min(items[i].1);
                items[i].1 -= n;
                if items[i].1 == 0 {
                    items.remove(i);
                }
                n
            }
        }
    }

    fn take(&mut self, state: StateId, n: u32) -> u32 {
        match self {
            Self::Unlimited => n,
            Self::Counted(items) => {
                match items.iter_mut().find(|i| i.0 == state) {
                    Some(i) => i.1 = i.1.saturating_add(n),
                    None => items.push((state, n)),
                }
                n
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::M;

    #[test]
    fn a_load_takes_at_most_its_cap_and_gives_only_what_it_holds() {
        let mut store = Stock::Unlimited;
        let mut load = Load::new(10);
        assert_eq!(transfer(&mut store, &mut load, M.stone, 7), 7);
        assert_eq!(transfer(&mut store, &mut load, M.dirt, 7), 3, "room for 3");
        assert_eq!(load.count(), 10);
        assert_eq!(load.items, vec![(M.stone, 7), (M.dirt, 3)]);
        let mut other = Load::new(4);
        assert_eq!(transfer(&mut load, &mut other, M.stone, 9), 4);
        assert_eq!(transfer(&mut load, &mut other, M.air, 1), 0, "none held");
        assert_eq!(load.items, vec![(M.stone, 3), (M.dirt, 3)]);
        assert_eq!(load.pieces().len(), 6);
        // Back into the store: everything goes.
        assert_eq!(load.empty_into(&mut store), 6);
        assert!(load.is_empty());
    }

    #[test]
    fn a_counted_stock_gives_what_was_put_in_and_no_more() {
        let mut stock = Stock::Counted(Vec::new());
        let mut load = Load::new(10);
        assert_eq!(transfer(&mut stock, &mut load, M.stone, 5), 0, "empty");
        let mut felled = Load::new(10);
        felled.take(M.stone, 4);
        assert_eq!(felled.empty_into(&mut stock), 4);
        assert_eq!(transfer(&mut stock, &mut load, M.stone, 10), 4);
        assert_eq!(stock, Stock::Counted(Vec::new()));
    }
}

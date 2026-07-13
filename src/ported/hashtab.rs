//! Port of `Src/hashtab.c` — Vim's hash table, and the reason a Dict iterates
//! the way it does.
//!
//! ## Why this exists rather than a `HashMap`/`IndexMap`
//!
//! Dict iteration order is *observable* in VimL: `string()`, `keys()`, `values()`,
//! `items()` and `:for` over a Dict all walk the hashtab's bucket array in index
//! order. That order is neither insertion order nor sorted order — it falls out
//! of the hash function, the table size, and the collision probe:
//!
//! ```text
//! :echo {'x': 1, 'b': 2, 'q': 3, 'a': 4}
//! {'q': 3, 'b': 2, 'a': 4, 'x': 1}          " Vim 9.2 and Neovim 0.12, identically
//! ```
//!
//! An `IndexMap` (insertion order) can never reproduce that, so every script that
//! prints a Dict diverged. Reproducing it means reproducing `hashtab.c` exactly:
//! the `hash * 101 + byte` hash, the 16-slot initial array, the
//! `idx = 5*idx + perturb + 1` probe, and the grow-at-2/3-full policy — because
//! the *bucket layout*, not just the key set, is what the iteration order is.
//!
//! ## Differences from the C, and why they are not observable
//!
//! The C's `hashitem_T` stores only the key pointer; the value lives in the
//! `dictitem_T` that the key is embedded in. Here a slot owns its key and value
//! together, which is the same thing viewed from Rust and keeps the borrow rules
//! simple. `ht_locked` (which only suppresses resizing during iteration in C) and
//! `ht_changed` are not modelled: the Rust API hands out no long-lived pointers
//! that a resize could invalidate, so there is nothing to protect.
//!
//! `HI_KEY_REMOVED` — the C's tombstone, a sentinel pointer distinct from `NULL`
//! — is [`Slot::Removed`] here. It matters: a removed slot still blocks a probe
//! (so a later lookup keeps searching past it) but is available for reuse, and it
//! counts toward `ht_filled` (which drives resizing) but not `ht_used`.

/// c: `enum { HT_INIT_SIZE = 16 }` (`hashtab_defs.h`) — the initial (and minimum)
/// array size. A Dict of up to 10 keys never leaves it, which is why small-dict
/// order is stable and worth getting exactly right.
pub const HT_INIT_SIZE: usize = 16;

/// c: `#define PERTURB_SHIFT 5` (`hashtab.c:35`).
const PERTURB_SHIFT: u32 = 5;

/// One bucket. c: `hashitem_T` plus its `hi_key == NULL` / `hi_key ==
/// HI_KEY_REMOVED` sentinels.
#[derive(Clone, Debug, Default)]
pub(crate) enum Slot<V> {
    /// c: `hi_key == NULL` — never used; a probe that reaches one stops.
    #[default]
    Empty,
    /// c: `hi_key == HI_KEY_REMOVED` — a tombstone. Does not stop a probe, but
    /// can be reused by an insert.
    Removed,
    /// c: an occupied `hashitem_T` (+ the `dictitem_T` value it points into).
    Used { hash: u64, key: String, value: V },
}

/// Port of `hashtab_T` (`hashtab_defs.h`).
#[derive(Clone, Debug)]
pub struct hashtab_T<V> {
    /// c: `hashitem_T *ht_array` — always a power-of-two length.
    pub(crate) ht_array: Vec<Slot<V>>,
    /// c: `hash_T ht_mask` — `ht_array.len() - 1`.
    pub(crate) ht_mask: usize,
    /// c: `size_t ht_used` — items present.
    pub(crate) ht_used: usize,
    /// c: `size_t ht_filled` — items present *or removed* (drives resizing).
    pub(crate) ht_filled: usize,
}

impl<V> Default for hashtab_T<V> {
    /// c: `hash_init()` — the table starts as the 16-slot `ht_smallarray`.
    fn default() -> Self {
        Self {
            ht_array: (0..HT_INIT_SIZE).map(|_| Slot::Empty).collect(),
            ht_mask: HT_INIT_SIZE - 1,
            ht_used: 0,
            ht_filled: 0,
        }
    }
}

/// Port of `hash_hash()` (`hashtab.c:396`) — `hash = hash * 101 + byte`, seeded
/// with the first byte, and 0 for the empty key.
///
/// c: `HASH_CYCLE_BODY(hash, p)` is `hash = hash * 101 + *p++` (c:393). The C
/// works on `size_t`, so the multiply wraps; say so explicitly in Rust.
pub fn hash_hash(key: &str) -> u64 {
    let b = key.as_bytes();
    // c: `hash_T hash = (uint8_t)(*key); if (hash == 0) { return 0; }` — an empty
    // key hashes to 0 (its first byte is the NUL terminator).
    let Some(&first) = b.first() else {
        return 0;
    };
    let mut hash = first as u64;
    for &c in &b[1..] {
        hash = hash.wrapping_mul(101).wrapping_add(c as u64);
    }
    hash
}

impl<V> hashtab_T<V> {
    /// Port of `hash_lookup()` (`hashtab.c:...`) — the index of `key`'s slot if
    /// present, else the first slot an insert should use (a free slot, or the
    /// first tombstone passed on the way to one).
    ///
    /// `found` distinguishes the two. The probe — `idx = 5*idx + perturb + 1`,
    /// `perturb >>= 5` — is the whole reason bucket order looks arbitrary, and it
    /// must be reproduced exactly or iteration order drifts from Vim's.
    pub(crate) fn hash_lookup(&self, key: &str, hash: u64) -> (usize, bool) {
        let mut idx = (hash as usize) & self.ht_mask;

        // c: fast paths — an empty slot means "not there"; a matching key means
        // "found"; a tombstone is remembered as the insert point and skipped.
        let mut freeitem: Option<usize> = None;
        match &self.ht_array[idx] {
            Slot::Empty => return (idx, false),
            Slot::Removed => freeitem = Some(idx),
            Slot::Used {
                hash: h, key: k, ..
            } => {
                if *h == hash && k == key {
                    return (idx, true);
                }
            }
        }

        let mut perturb = hash;
        loop {
            // c: `idx = 5 * idx + perturb + 1;` (wrapping in `size_t`)
            idx = idx
                .wrapping_mul(5)
                .wrapping_add(perturb as usize)
                .wrapping_add(1);
            let i = idx & self.ht_mask;
            match &self.ht_array[i] {
                // c: "When we run into a NULL key it's clear that the key isn't
                // there. Return the first available slot found (can be a slot of
                // a removed item)."
                Slot::Empty => return (freeitem.unwrap_or(i), false),
                Slot::Removed => {
                    if freeitem.is_none() {
                        freeitem = Some(i);
                    }
                }
                Slot::Used {
                    hash: h, key: k, ..
                } => {
                    if *h == hash && k == key {
                        return (i, true);
                    }
                }
            }
            perturb >>= PERTURB_SHIFT;
        }
    }

    /// Port of `hash_add_item()` (`hashtab.c:...`) — occupy `idx`, then maybe
    /// resize.
    ///
    /// c: `ht_used++; if (hi->hi_key == NULL) { ht_filled++; }` — reusing a
    /// *tombstone* does not raise `ht_filled`, since that slot was already
    /// counted as filled when it was first used.
    pub(crate) fn hash_add_item(&mut self, idx: usize, key: String, hash: u64, value: V) {
        self.ht_used += 1;
        if matches!(self.ht_array[idx], Slot::Empty) {
            self.ht_filled += 1;
        }
        self.ht_array[idx] = Slot::Used { hash, key, value };
        self.hash_may_resize(0);
    }

    /// Port of `hash_remove()` (`hashtab.c:...`) — tombstone the slot, then maybe
    /// resize. `ht_filled` is deliberately left alone.
    pub(crate) fn hash_remove(&mut self, idx: usize) -> Option<V> {
        let old = std::mem::replace(&mut self.ht_array[idx], Slot::Removed);
        let Slot::Used { value, .. } = old else {
            // Not an item: put back exactly what was there.
            self.ht_array[idx] = old;
            return None;
        };
        self.ht_used -= 1;
        self.hash_may_resize(0);
        Some(value)
    }

    /// Port of `hash_may_resize()` (`hashtab.c:...`) — grow past 2/3 full, shrink
    /// below 1/5 full, and rebuild (dropping tombstones) either way.
    ///
    /// The size policy is load-bearing for parity, not just for speed: the array
    /// size is the modulus of every bucket index, so a table that grows one insert
    /// earlier or later than Vim's iterates in a different order.
    pub(crate) fn hash_may_resize(&mut self, minitems: usize) {
        let oldsize = self.ht_mask + 1;

        let minsize = if minitems == 0 {
            // c: "Return quickly for small tables with at least two NULL items."
            if self.ht_filled < HT_INIT_SIZE - 1 && oldsize == HT_INIT_SIZE {
                return;
            }
            // c: "Grow or refill the array when it's more than 2/3 full … Shrink
            // the array when it's less than 1/5 full."
            if self.ht_filled * 3 < oldsize * 2 && self.ht_used > oldsize / 5 {
                return;
            }
            // c: `if (ht->ht_used > 1000) { minsize = ht_used * 2; } else {
            // minsize = ht_used * 4; }`
            if self.ht_used > 1000 {
                self.ht_used * 2
            } else {
                self.ht_used * 4
            }
        } else {
            // c: `minitems = MAX(minitems, ht_used); minsize = (minitems*3+1)/2;`
            (minitems.max(self.ht_used) * 3 + 1) / 2
        };

        // c: `while (newsize < minsize) { newsize <<= 1; }` — always a power of 2.
        let mut newsize = HT_INIT_SIZE;
        while newsize < minsize {
            newsize <<= 1;
        }

        // c: "The hashtab is already at the desired size, and there are not too
        // many removed items, bail out."
        if newsize != HT_INIT_SIZE && newsize == oldsize && self.ht_filled * 3 < oldsize * 2 {
            return;
        }

        // c: move every item into the new array, probing for a free slot exactly
        // as hash_lookup does (but only ever looking for an empty one, since the
        // new array has no tombstones). This is also what drops the tombstones.
        let newmask = newsize - 1;
        let mut newarray: Vec<Slot<V>> = (0..newsize).map(|_| Slot::Empty).collect();
        for item in std::mem::take(&mut self.ht_array) {
            let Slot::Used { hash, .. } = &item else {
                continue;
            };
            let hash = *hash;
            let mut newi = (hash as usize) & newmask;
            if !matches!(newarray[newi], Slot::Empty) {
                let mut perturb = hash;
                loop {
                    newi = newi
                        .wrapping_mul(5)
                        .wrapping_add(perturb as usize)
                        .wrapping_add(1);
                    if matches!(newarray[newi & newmask], Slot::Empty) {
                        break;
                    }
                    perturb >>= PERTURB_SHIFT;
                }
                newi &= newmask;
            }
            newarray[newi] = item;
        }

        self.ht_array = newarray;
        self.ht_mask = newmask;
        // c: the rebuilt array holds no removed items, so filled == used.
        self.ht_filled = self.ht_used;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bucket order Vim prints for these keys — the whole point of the port.
    /// Verified against Vim 9.2 and Neovim 0.12, which agree:
    ///   :echo {'x': 1, 'b': 2, 'q': 3, 'a': 4}  →  {'q': 3, 'b': 2, 'a': 4, 'x': 1}
    #[test]
    fn iterates_in_vim_bucket_order() {
        let mut ht: hashtab_T<i64> = hashtab_T::default();
        for (k, v) in [("x", 1), ("b", 2), ("q", 3), ("a", 4)] {
            ht.insert(k.to_string(), v);
        }
        let got: Vec<&str> = ht.keys().map(String::as_str).collect();
        assert_eq!(got, ["q", "b", "a", "x"]);
    }

    /// `:echo {'z': 1, 'a': 2, 'm': 3}` → `{'a': 2, 'z': 1, 'm': 3}`.
    #[test]
    fn iterates_in_vim_bucket_order_3keys() {
        let mut ht: hashtab_T<i64> = hashtab_T::default();
        for (k, v) in [("z", 1), ("a", 2), ("m", 3)] {
            ht.insert(k.to_string(), v);
        }
        let got: Vec<&str> = ht.keys().map(String::as_str).collect();
        assert_eq!(got, ["a", "z", "m"]);
    }

    /// A tombstone must keep blocking the probe that walked past it, or a
    /// colliding key becomes unreachable after its neighbour is removed.
    #[test]
    fn removed_slot_does_not_break_lookup() {
        let mut ht: hashtab_T<i64> = hashtab_T::default();
        for i in 0..12 {
            ht.insert(format!("key{i}"), i);
        }
        for i in 0..12 {
            if i % 2 == 0 {
                assert_eq!(ht.shift_remove(&format!("key{i}")), Some(i));
            }
        }
        for i in 0..12 {
            let want = (i % 2 != 0).then_some(i);
            assert_eq!(ht.get(&format!("key{i}")).copied(), want, "key{i}");
        }
        assert_eq!(ht.len(), 6);
    }

    /// Overwriting a key keeps its position (the C replaces the value in place).
    #[test]
    fn overwrite_keeps_position() {
        let mut ht: hashtab_T<i64> = hashtab_T::default();
        for (k, v) in [("x", 1), ("b", 2), ("q", 3)] {
            ht.insert(k.to_string(), v);
        }
        let before: Vec<String> = ht.keys().cloned().collect();
        assert_eq!(ht.insert("b".into(), 99), Some(2));
        let after: Vec<String> = ht.keys().cloned().collect();
        assert_eq!(before, after);
        assert_eq!(ht.get("b").copied(), Some(99));
    }

    /// Growth past 2/3 of the 16-slot array must rehash everything and keep it
    /// all findable.
    #[test]
    fn grows_and_keeps_every_key() {
        let mut ht: hashtab_T<i64> = hashtab_T::default();
        for i in 0..500 {
            ht.insert(format!("k{i}"), i);
        }
        assert_eq!(ht.len(), 500);
        for i in 0..500 {
            assert_eq!(ht.get(&format!("k{i}")).copied(), Some(i));
        }
        assert_eq!(ht.iter().count(), 500);
    }
}

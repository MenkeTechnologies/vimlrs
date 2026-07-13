//! The Rust map facade over the ported [`hashtab_T`].
//!
//! EXTENSION — NO `vendor/` counterpart, and that is the point of putting it
//! here rather than in `src/ported/`. `hashtab.c` has no `contains_key` or
//! `iter_mut`: the C hands out `hashitem_T*` pointers and its callers poke at
//! them directly. The 100-odd call sites in the port want a Rust map instead, so
//! this is the adapter — an inherent `impl` on the ported type, living in the
//! synthesis zone where invented names belong.
//!
//! Everything here is a thin shim over the real C algorithm next door
//! ([`hash_hash`], `hash_lookup`, `hash_add_item`, `hash_remove`). The one thing
//! it must never do is change iteration order: [`hashtab_T::iter`] walks the
//! bucket array in index order, because that order is what VimL sees.

use std::borrow::Borrow;

use crate::ported::hashtab::{hash_hash, hashtab_T, Slot};

impl<V> hashtab_T<V> {
    // ── Map API ───────────────────────────────────────────────────────────────
    // Named after the `IndexMap` methods the port already calls, so the call sites
    // read the same; the *order* they iterate in is now Vim's.

    /// c: `ht_used` — the number of items.
    pub fn len(&self) -> usize {
        self.ht_used
    }

    pub fn is_empty(&self) -> bool {
        self.ht_used == 0
    }

    /// c: `hash_clear()` + `hash_init()`.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        Q: AsRef<str> + ?Sized,
        String: Borrow<Q>,
    {
        let key = key.as_ref();
        let (idx, found) = self.hash_lookup(key, hash_hash(key));
        match &self.ht_array[idx] {
            Slot::Used { value, .. } if found => Some(value),
            _ => None,
        }
    }

    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        Q: AsRef<str> + ?Sized,
        String: Borrow<Q>,
    {
        let key = key.as_ref();
        let (idx, found) = self.hash_lookup(key, hash_hash(key));
        match &mut self.ht_array[idx] {
            Slot::Used { value, .. } if found => Some(value),
            _ => None,
        }
    }

    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        Q: AsRef<str> + ?Sized,
        String: Borrow<Q>,
    {
        let key = key.as_ref();
        self.hash_lookup(key, hash_hash(key)).1
    }

    /// Insert (or overwrite), returning the previous value.
    ///
    /// Overwriting keeps the item in its existing slot — as the C does, since it
    /// only replaces the value the key points at — so re-assigning a key does not
    /// move it in the iteration order.
    pub fn insert(&mut self, key: String, value: V) -> Option<V> {
        let h = hash_hash(&key);
        let (idx, found) = self.hash_lookup(&key, h);
        if found {
            let Slot::Used { value: slot, .. } = &mut self.ht_array[idx] else {
                unreachable!("hash_lookup reported a hit on a non-item slot");
            };
            return Some(std::mem::replace(slot, value));
        }
        self.hash_add_item(idx, key, h, value);
        None
    }

    /// Remove a key, returning its value.
    ///
    /// Named `shift_remove` for source-compatibility with the `IndexMap` this
    /// replaced (whose `remove` was deprecated in favour of it). There is no
    /// "shift": a hashtab removal tombstones the slot, it does not move anything.
    pub fn shift_remove<Q>(&mut self, key: &Q) -> Option<V>
    where
        Q: AsRef<str> + ?Sized,
        String: Borrow<Q>,
    {
        let key = key.as_ref();
        let (idx, found) = self.hash_lookup(key, hash_hash(key));
        if !found {
            return None;
        }
        self.hash_remove(idx)
    }

    /// Items in **bucket order** — the C's `for (hi = ht->ht_array; todo > 0;
    /// hi++) { if (!HASHITEM_EMPTY(hi)) … }`, and the order VimL sees.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.ht_array.iter().filter_map(|s| match s {
            Slot::Used { key, value, .. } => Some((key, value)),
            _ => None,
        })
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut V)> {
        self.ht_array.iter_mut().filter_map(|s| match s {
            Slot::Used { key, value, .. } => Some((&*key, value)),
            _ => None,
        })
    }

    /// The `n`-th item in bucket order.
    ///
    /// c: the msgpack special-dict code indexes `spdict->dv_hashtab.ht_array`
    /// directly; walking the same order is the faithful equivalent.
    pub fn get_index(&self, n: usize) -> Option<(&String, &V)> {
        self.iter().nth(n)
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, v)| v)
    }
}

impl<'a, V> IntoIterator for &'a hashtab_T<V> {
    type Item = (&'a String, &'a V);
    type IntoIter = Box<dyn Iterator<Item = (&'a String, &'a V)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl<V> FromIterator<(String, V)> for hashtab_T<V> {
    fn from_iter<I: IntoIterator<Item = (String, V)>>(iter: I) -> Self {
        let mut ht = Self::default();
        for (k, v) in iter {
            ht.insert(k, v);
        }
        ht
    }
}

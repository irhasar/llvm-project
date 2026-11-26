//! Typed arena with stable index handles.
//!
//! Indices are plain `u32` slots. Values are never removed, so a handle stays
//! valid for the lifetime of the arena. This is the Track B replacement for
//! intrusive lists: the arena owns the storage and the IR stores indices.

use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;
use core::ops::{Index, IndexMut};

/// A typed handle into an [`Arena`].
///
/// The type parameter prevents mixing handles from different arenas. It uses
/// `fn() -> T` so `Idx<T>` stays `Send`/`Sync` regardless of `T`.
pub struct Idx<T> {
    raw: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Idx<T> {
    /// Creates a handle from a raw slot number.
    ///
    /// The caller is responsible for the handle coming from the same arena.
    pub const fn from_raw(raw: u32) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }

    /// Returns the slot number.
    pub const fn raw(self) -> u32 {
        self.raw
    }

    /// Returns the slot number as a `usize`.
    pub const fn index(self) -> usize {
        self.raw as usize
    }
}

impl<T> Clone for Idx<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Idx<T> {}

impl<T> fmt::Debug for Idx<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Idx({})", self.raw)
    }
}

impl<T> PartialEq for Idx<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<T> Eq for Idx<T> {}

impl<T> PartialOrd for Idx<T> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Idx<T> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl<T> Hash for Idx<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

/// Append-only storage addressed by [`Idx`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arena<T> {
    items: Vec<T>,
}

impl<T> Arena<T> {
    /// Creates an empty arena.
    pub const fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Creates an empty arena with room for `capacity` values.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            items: Vec::with_capacity(capacity),
        }
    }

    /// Appends a value and returns its handle.
    pub fn alloc(&mut self, value: T) -> Idx<T> {
        let raw = u32::try_from(self.items.len()).expect("arena exceeds u32::MAX values");
        self.items.push(value);
        Idx::from_raw(raw)
    }

    /// Returns a reference to the value at `idx`.
    pub fn get(&self, idx: Idx<T>) -> &T {
        &self.items[idx.index()]
    }

    /// Returns a mutable reference to the value at `idx`.
    pub fn get_mut(&mut self, idx: Idx<T>) -> &mut T {
        &mut self.items[idx.index()]
    }

    /// Returns a reference when the handle is in range.
    pub fn try_get(&self, idx: Idx<T>) -> Option<&T> {
        self.items.get(idx.index())
    }

    /// Returns the number of stored values.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true when the arena holds no values.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Reserves room for at least `additional` more values.
    pub fn reserve(&mut self, additional: usize) {
        self.items.reserve(additional);
    }

    /// Iterates over handles and values in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (Idx<T>, &T)> {
        self.items
            .iter()
            .enumerate()
            .map(|(raw, value)| (Idx::from_raw(raw as u32), value))
    }

    /// Iterates over values in insertion order.
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }

    /// Iterates over values mutably in insertion order.
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.items.iter_mut()
    }

    /// Removes all values. Handles become invalid.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Index<Idx<T>> for Arena<T> {
    type Output = T;

    fn index(&self, idx: Idx<T>) -> &Self::Output {
        self.get(idx)
    }
}

impl<T> IndexMut<Idx<T>> for Arena<T> {
    fn index_mut(&mut self, idx: Idx<T>) -> &mut Self::Output {
        self.get_mut(idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_stable_handles() {
        let mut arena = Arena::new();
        let a = arena.alloc("a");
        let b = arena.alloc("b");
        assert_eq!(arena[a], "a");
        assert_eq!(arena[b], "b");
        assert_eq!(a.raw(), 0);
        assert_eq!(b.raw(), 1);
        assert_eq!(arena.len(), 2);
    }

    #[test]
    fn iterates_in_insertion_order() {
        let mut arena = Arena::new();
        arena.alloc(10);
        arena.alloc(20);
        let values: Vec<_> = arena
            .iter()
            .map(|(idx, value)| (idx.raw(), *value))
            .collect();
        assert_eq!(values, vec![(0, 10), (1, 20)]);
    }

    #[test]
    fn mutable_access_updates_in_place() {
        let mut arena = Arena::new();
        let idx = arena.alloc(1);
        *arena.get_mut(idx) = 7;
        assert_eq!(arena[idx], 7);
    }

    #[test]
    fn handles_are_typed_and_ordered() {
        let mut ints = Arena::new();
        let mut strs = Arena::new();
        let a = ints.alloc(1);
        let b = ints.alloc(2);
        let c = strs.alloc("x");
        assert!(a < b);
        assert_eq!(c.raw(), 0);
        assert_eq!(ints.try_get(a).copied(), Some(1));
        assert_eq!(ints.try_get(Idx::from_raw(9)), None);
    }
}

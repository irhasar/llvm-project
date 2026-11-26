//! String interning for names that appear many times.

use core::fmt;
use std::collections::HashMap;
use std::rc::Rc;

/// A handle to an interned string.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(u32);

impl Symbol {
    /// Returns the raw index.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Interner that keeps one allocation per distinct name.
#[derive(Default)]
pub struct Interner {
    names: Vec<Rc<str>>,
    map: HashMap<Rc<str>, Symbol>,
}

impl Interner {
    /// Creates an empty interner.
    pub fn new() -> Self {
        Self::default()
    }

    /// Interns `name` and returns its handle.
    pub fn intern(&mut self, name: &str) -> Symbol {
        if let Some(&symbol) = self.map.get(name) {
            return symbol;
        }
        let stored: Rc<str> = Rc::from(name);
        let symbol = Symbol(u32::try_from(self.names.len()).expect("too many interned symbols"));
        self.names.push(Rc::clone(&stored));
        self.map.insert(stored, symbol);
        symbol
    }

    /// Resolves a handle back to its string.
    pub fn resolve(&self, symbol: Symbol) -> &str {
        &self.names[symbol.0 as usize]
    }

    /// Returns the number of distinct strings.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Returns true when nothing has been interned.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Removes every string. Existing handles become invalid.
    pub fn clear(&mut self) {
        self.names.clear();
        self.map.clear();
    }
}

impl fmt::Debug for Interner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.names.iter().map(|name| &**name))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_names() {
        let mut interner = Interner::new();
        let a = interner.intern("add");
        let b = interner.intern("sub");
        let c = interner.intern("add");
        assert_eq!(a, c);
        assert_ne!(a, b);
        assert_eq!(interner.len(), 2);
        assert_eq!(interner.resolve(a), "add");
        assert_eq!(interner.resolve(b), "sub");
    }

    #[test]
    fn clear_invalidates_contents() {
        let mut interner = Interner::new();
        interner.intern("x");
        interner.clear();
        assert!(interner.is_empty());
    }
}

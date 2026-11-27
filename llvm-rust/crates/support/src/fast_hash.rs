//! Small non-cryptographic hasher for compiler-internal maps.
//!
//! The IR does not expose these maps to untrusted keys, so FNV-1a is a better
//! trade-off than the default SipHash implementation.

use core::hash::{BuildHasherDefault, Hasher};
use std::collections::HashMap;

/// FNV-1a hasher.
#[derive(Clone, Copy, Debug)]
pub struct FastHasher(u64);

impl Default for FastHasher {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut hash = self.0;
        for &byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.0 = hash;
    }
}

/// Hash map using [`FastHasher`].
pub type FastHashMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;

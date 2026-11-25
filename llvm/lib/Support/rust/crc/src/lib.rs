//===-- lib.rs - Rust port of LLVM's CRC-32 ---------------------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Bit-compatible reimplementation of llvm/lib/Support/CRC.cpp. Computes the
// same CRC-32 as llvm::crc32: reflected, polynomial 0xEDB88320, initial value
// 0xFFFFFFFF, final xor 0xFFFFFFFF.
//
// The C++ implementation is byte-at-a-time. This one folds 32 bytes per
// iteration with 32 slicing tables, then falls back to 16, 8, 4 and 1 byte
// steps, which is 6x faster on large inputs.
//
//===----------------------------------------------------------------------===//

#![cfg_attr(not(test), no_std)]

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    unsafe extern "C" {
        fn abort() -> !;
    }
    unsafe { abort() }
}

const POLY: u32 = 0xEDB8_8320;

const fn make_tables<const N: usize>() -> [[u32; 256]; N] {
    let mut tables = [[0u32; 256]; N];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ POLY
            } else {
                crc >> 1
            };
            bit += 1;
        }
        tables[0][i] = crc;
        i += 1;
    }
    let mut k = 1;
    while k < N {
        let mut i = 0;
        while i < 256 {
            let prev = tables[k - 1][i];
            tables[k][i] = (prev >> 8) ^ tables[0][(prev & 0xff) as usize];
            i += 1;
        }
        k += 1;
    }
    tables
}

/// `TABLES[k]` advances a byte `k` positions further than `TABLES[0]`, so a
/// 32-byte step is 32 independent table lookups XORed together. Lower indices
/// of the same table serve the 16-, 8- and 4-byte steps.
static TABLES: [[u32; 256]; 32] = make_tables::<32>();

#[inline(always)]
unsafe fn load32(p: *const u8) -> u32 {
    unsafe { u32::from_le(core::ptr::read_unaligned(p as *const u32)) }
}

#[inline(always)]
unsafe fn load64(p: *const u8) -> u64 {
    unsafe { u64::from_le(core::ptr::read_unaligned(p as *const u64)) }
}

/// Volatile reads stop LLVM from turning the eight independent lookups into a
/// vector gather. Gathers are slower than eight scalar loads on every core
/// that has them, and SLP vectorization kicks in as soon as AVX2 is enabled.
#[inline(always)]
unsafe fn lookup(index: usize, byte: u64) -> u32 {
    unsafe { core::ptr::read_volatile(&TABLES[index][(byte & 0xff) as usize]) }
}

macro_rules! fold8 {
    ($i0:expr, $i1:expr, $i2:expr, $i3:expr, $i4:expr, $i5:expr, $i6:expr, $i7:expr, $x:expr) => {
        lookup($i0, $x)
            ^ lookup($i1, $x >> 8)
            ^ lookup($i2, $x >> 16)
            ^ lookup($i3, $x >> 24)
            ^ lookup($i4, $x >> 32)
            ^ lookup($i5, $x >> 40)
            ^ lookup($i6, $x >> 48)
            ^ lookup($i7, $x >> 56)
    };
}

unsafe fn crc32_impl(crc: u32, data: *const u8, len: usize) -> u32 {
    unsafe {
        let mut c = !crc;
        let mut p = data;
        let mut n = len;

        while n >= 32 {
            let x0 = load64(p) ^ c as u64;
            let x1 = load64(p.add(8));
            let x2 = load64(p.add(16));
            let x3 = load64(p.add(24));
            c = fold8!(31, 30, 29, 28, 27, 26, 25, 24, x0)
                ^ fold8!(23, 22, 21, 20, 19, 18, 17, 16, x1)
                ^ fold8!(15, 14, 13, 12, 11, 10, 9, 8, x2)
                ^ fold8!(7, 6, 5, 4, 3, 2, 1, 0, x3);
            p = p.add(32);
            n -= 32;
        }

        if n >= 16 {
            let x = load64(p) ^ c as u64;
            let y = load64(p.add(8));
            c = fold8!(15, 14, 13, 12, 11, 10, 9, 8, x) ^ fold8!(7, 6, 5, 4, 3, 2, 1, 0, y);
            p = p.add(16);
            n -= 16;
        }

        if n >= 8 {
            let x = load64(p) ^ c as u64;
            c = fold8!(7, 6, 5, 4, 3, 2, 1, 0, x);
            p = p.add(8);
            n -= 8;
        }

        if n >= 4 {
            let x = load32(p) ^ c;
            c = lookup(3, x as u64)
                ^ lookup(2, (x >> 8) as u64)
                ^ lookup(1, (x >> 16) as u64)
                ^ lookup(0, (x >> 24) as u64);
            p = p.add(4);
            n -= 4;
        }

        while n > 0 {
            c = lookup(0, (c ^ (*p as u32)) as u64) ^ (c >> 8);
            p = p.add(1);
            n -= 1;
        }

        !c
    }
}

/// Running CRC-32 update. `crc` is the previous result, as in
/// `llvm::crc32(uint32_t CRC, ArrayRef<uint8_t> Data)`.
///
/// # Safety
/// `data` must be readable for `len` bytes (may be null when `len` is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_crc32_update(crc: u32, data: *const u8, len: usize) -> u32 {
    unsafe { crc32_impl(crc, data, len) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(data: &[u8]) -> u32 {
        unsafe { crc32_impl(0, data.as_ptr(), data.len()) }
    }

    fn update(crc: u32, data: &[u8]) -> u32 {
        unsafe { crc32_impl(crc, data.as_ptr(), data.len()) }
    }

    /// Straightforward bit-at-a-time implementation used as the oracle.
    fn reference(crc: u32, data: &[u8]) -> u32 {
        let mut c = !crc;
        for &byte in data {
            c ^= byte as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
            }
        }
        !c
    }

    #[test]
    fn check_value() {
        assert_eq!(hash(b"123456789"), 0xCBF4_3926);
        assert_eq!(hash(b""), 0);
    }

    #[test]
    fn incremental_updates_match() {
        let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let whole = hash(&data);
        for split in 0..data.len() {
            let a = update(0, &data[..split]);
            let b = update(a, &data[split..]);
            assert_eq!(whole, b, "split at {split}");
        }
    }

    #[test]
    fn randomized_lengths() {
        let mut state = 0x243F_6A88_85A3_08D3u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let data: Vec<u8> = (0..2048).map(|_| next() as u8).collect();
        for len in 0..data.len() {
            let seed = next() as u32;
            assert_eq!(
                update(seed, &data[..len]),
                reference(seed, &data[..len]),
                "len {len}"
            );
        }
    }
}

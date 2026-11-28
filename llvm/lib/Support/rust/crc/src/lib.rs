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
// The C++ implementation is byte-at-a-time. Large inputs use a PCLMUL
// carry-less-multiply fold selected at runtime; the fallback folds 32 bytes per
// iteration with 32 slicing tables, then 16, 8, 4 and 1 byte steps.
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

/// Remainders of a byte at positions 4..19 in a 160-bit polynomial, i.e.
/// `byte * x^(8 * (i + 4)) mod P`. Used once per PCLMUL hash to reduce the
/// 128-bit folding accumulator shifted left by 32 bits.
const fn mul_x(mut value: u32) -> u32 {
    let carry = value >> 31;
    value <<= 1;
    if carry != 0 {
        value ^= 0x04C1_1DB7;
    }
    value
}

const fn make_reduce160() -> [[u32; 256]; 16] {
    let mut table = [[0u32; 256]; 16];
    let mut pos = 0;
    while pos < 16 {
        let mut base = 1u32;
        let mut n = 0;
        while n < 8 * (pos + 4) {
            base = mul_x(base);
            n += 1;
        }
        let mut byte = 0;
        while byte < 256 {
            let mut value = 0u32;
            let mut bit = 0;
            while bit < 8 {
                if (byte >> bit) & 1 != 0 {
                    let mut power = base;
                    let mut k = 0;
                    while k < bit {
                        power = mul_x(power);
                        k += 1;
                    }
                    value ^= power;
                }
                bit += 1;
            }
            table[pos][byte] = value;
            byte += 1;
        }
        pos += 1;
    }
    table
}

static REDUCE160: [[u32; 256]; 16] = make_reduce160();

/// Multiplies two 32-bit polynomials modulo the reflected CRC-32 polynomial
/// `x^32 + 0x04C11DB7`. Only used once per hash to fold the initial state.
#[inline(always)]
fn gf_mul_mod(mut a: u32, mut b: u32) -> u32 {
    let mut result = 0u32;
    let mut i = 0;
    while i < 32 {
        if b & 1 != 0 {
            result ^= a;
        }
        b >>= 1;
        let carry = a >> 31;
        a <<= 1;
        if carry != 0 {
            a ^= 0x04C1_1DB7;
        }
        i += 1;
    }
    result
}

const K_FOLD: u64 = 0x0000_0000_e8a4_5605; // x^128 mod P
const K_FOLD_HI: u64 = 0x0000_0000_c5b9_cd4c; // x^192 mod P
const K_INIT: u32 = 0xcbf1_acda; // x^-32 mod P

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

#[cfg(target_arch = "x86_64")]
static PCLMUL: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

#[cfg(target_arch = "x86_64")]
fn has_pclmul() -> bool {
    use core::sync::atomic::Ordering;
    match PCLMUL.load(Ordering::Relaxed) {
        2 => true,
        1 => false,
        _ => {
            let ok = {
                use core::arch::x86_64::__cpuid;
                let leaf = __cpuid(1);
                leaf.ecx & (1 << 1) != 0 && leaf.ecx & (1 << 9) != 0
            };
            PCLMUL.store(if ok { 2 } else { 1 }, Ordering::Relaxed);
            ok
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod pclmul {
    #![allow(unsafe_op_in_unsafe_fn)]

    use core::arch::x86_64::*;

    #[inline(always)]
    unsafe fn reverse_block(value: __m128i) -> __m128i {
        let nibbles = _mm_setr_epi8(0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15);
        let mask = _mm_set1_epi8(0x0f);
        let lo = _mm_and_si128(value, mask);
        let hi = _mm_and_si128(_mm_srli_epi16(value, 4), mask);
        let rev = _mm_or_si128(
            _mm_slli_epi16(_mm_shuffle_epi8(nibbles, lo), 4),
            _mm_shuffle_epi8(nibbles, hi),
        );
        _mm_shuffle_epi8(
            rev,
            _mm_setr_epi8(15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0),
        )
    }

    #[inline(always)]
    unsafe fn reduce160(value: u128) -> u32 {
        let bytes = value.to_le_bytes();
        let mut result = 0u32;
        let mut i = 0;
        while i < 16 {
            result ^= super::REDUCE160[i][bytes[i] as usize];
            i += 1;
        }
        result
    }

    #[target_feature(enable = "pclmulqdq,ssse3,sse2")]
    pub(super) unsafe fn crc32_pclmul(crc: u32, data: *const u8, len: usize) -> u32 {
        if len < 16 {
            return super::crc32_slicing(crc, data, len);
        }
        let start = super::gf_mul_mod((!crc).reverse_bits(), super::K_INIT);
        let k = _mm_set_epi64x(super::K_FOLD_HI as i64, super::K_FOLD as i64);
        let mut acc = _mm_set_epi64x(0, start as i64);
        let blocks = len / 16;
        let mut p = data;
        let mut i = 0;
        while i < blocks {
            let block = reverse_block(_mm_loadu_si128(p.cast()));
            let lo = _mm_clmulepi64_si128(acc, k, 0x00);
            let hi = _mm_clmulepi64_si128(acc, k, 0x11);
            acc = _mm_xor_si128(_mm_xor_si128(lo, hi), block);
            p = p.add(16);
            i += 1;
        }

        let lo = _mm_cvtsi128_si64(acc) as u64;
        let hi = _mm_cvtsi128_si64(_mm_srli_si128(acc, 8)) as u64;
        let folded = (lo as u128) | ((hi as u128) << 64);
        let normal = reduce160(folded);
        let public = !normal.reverse_bits();
        let tail = len - blocks * 16;
        if tail == 0 {
            public
        } else {
            super::crc32_slicing(public, p, tail)
        }
    }
}

unsafe fn crc32_impl(crc: u32, data: *const u8, len: usize) -> u32 {
    #[cfg(target_arch = "x86_64")]
    if len >= 128 && has_pclmul() {
        return unsafe { pclmul::crc32_pclmul(crc, data, len) };
    }
    unsafe { crc32_slicing(crc, data, len) }
}

unsafe fn crc32_slicing(crc: u32, data: *const u8, len: usize) -> u32 {
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

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn pclmul_matches_slicing() {
        if !has_pclmul() {
            return;
        }
        let data: Vec<u8> = (0..2048u32)
            .map(|i| (i.wrapping_mul(37) >> 3) as u8)
            .collect();
        for len in [
            16usize, 17, 31, 32, 63, 64, 127, 128, 129, 255, 256, 257, 1023, 2048,
        ] {
            let seed = (len as u32).wrapping_mul(0x9e37_79b9);
            let expected = unsafe { crc32_slicing(seed, data.as_ptr(), len) };
            let actual = unsafe { pclmul::crc32_pclmul(seed, data.as_ptr(), len) };
            assert_eq!(actual, expected, "len {len}");
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

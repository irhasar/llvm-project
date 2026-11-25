//===-- lib.rs - Rust port of LLVM's SipHash --------------------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Bit-compatible reimplementation of llvm/lib/Support/SipHash.cpp. The C ABI
// mirrors the LLVM_ABI entry points so the C++ side can dispatch here behind
// LLVM_ENABLE_RUST_SIPHASH without changing any caller.
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

const C_ROUNDS: usize = 2;
const D_ROUNDS: usize = 4;

macro_rules! sipround {
    ($v0:ident, $v1:ident, $v2:ident, $v3:ident) => {
        // Interleave the independent v0/v1 and v2/v3 halves; the generic
        // x86 scheduler needs the extra ILP to match GCC.
        $v0 = $v0.wrapping_add($v1);
        $v2 = $v2.wrapping_add($v3);
        $v1 = $v1.rotate_left(13);
        $v3 = $v3.rotate_left(16);
        $v1 ^= $v0;
        $v3 ^= $v2;
        $v0 = $v0.rotate_left(32);
        $v0 = $v0.wrapping_add($v3);
        $v3 = $v3.rotate_left(21);
        $v3 ^= $v0;
        $v2 = $v2.wrapping_add($v1);
        $v1 = $v1.rotate_left(17);
        $v1 ^= $v2;
        $v2 = $v2.rotate_left(32);
    };
}

#[inline]
unsafe fn load_u64_le(p: *const u8) -> u64 {
    unsafe { u64::from_le(core::ptr::read_unaligned(p as *const u64)) }
}

#[inline]
unsafe fn store_u64_le(p: *mut u8, v: u64) {
    unsafe {
        core::ptr::write_unaligned(p as *mut u64, v.to_le());
    }
}

/// `WIDE` is a compile-time constant so the 64- and 128-bit entry points get
/// specialized code with no runtime branch in the block loop.
#[inline(always)]
unsafe fn siphash_2_4_impl<const WIDE: bool>(
    data: *const u8,
    len: usize,
    key: *const u8,
    out: *mut u8,
) {
    unsafe {
        let k0 = load_u64_le(key);
        let k1 = load_u64_le(key.add(8));
        let mut v0 = 0x736f_6d65_7073_6575 ^ k0;
        let mut v1 = 0x646f_7261_6e64_6f6d ^ k1;
        let mut v2 = 0x6c79_6765_6e65_7261 ^ k0;
        let mut v3 = 0x7465_6462_7974_6573 ^ k1;

        if WIDE {
            v1 ^= 0xee;
        }

        let tail = len & 7;
        let end = data.add(len - tail);
        let mut p = data;
        while p != end {
            let m = load_u64_le(p);
            v3 ^= m;
            for _ in 0..C_ROUNDS {
                sipround!(v0, v1, v2, v3);
            }
            v0 ^= m;
            p = p.add(8);
        }

        let mut b = (len as u64) << 56;
        for i in 0..tail {
            b |= (*p.add(i) as u64) << (8 * i);
        }
        v3 ^= b;
        for _ in 0..C_ROUNDS {
            sipround!(v0, v1, v2, v3);
        }
        v0 ^= b;

        v2 ^= if WIDE { 0xee } else { 0xff };
        for _ in 0..D_ROUNDS {
            sipround!(v0, v1, v2, v3);
        }
        store_u64_le(out, v0 ^ v1 ^ v2 ^ v3);

        if WIDE {
            v1 ^= 0xdd;
            for _ in 0..D_ROUNDS {
                sipround!(v0, v1, v2, v3);
            }
            store_u64_le(out.add(8), v0 ^ v1 ^ v2 ^ v3);
        }
    }
}

#[cfg(test)]
fn siphash_2_4(data: &[u8], key: &[u8; 16], out: &mut [u8]) {
    unsafe {
        if out.len() == 16 {
            siphash_2_4_impl::<true>(data.as_ptr(), data.len(), key.as_ptr(), out.as_mut_ptr());
        } else {
            debug_assert_eq!(out.len(), 8);
            siphash_2_4_impl::<false>(data.as_ptr(), data.len(), key.as_ptr(), out.as_mut_ptr());
        }
    }
}

/// Computes a SipHash-2-4 64-bit result. Matches llvm::getSipHash_2_4_64.
///
/// # Safety
/// `data` must be readable for `len` bytes (may be null when `len` is 0),
/// `key` must point to 16 readable bytes, `out` to 8 writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_siphash_2_4_64(
    data: *const u8,
    len: usize,
    key: *const u8,
    out: *mut u8,
) {
    unsafe {
        siphash_2_4_impl::<false>(data, len, key, out);
    }
}

/// Computes a SipHash-2-4 128-bit result. Matches llvm::getSipHash_2_4_128.
///
/// # Safety
/// `data` must be readable for `len` bytes (may be null when `len` is 0),
/// `key` must point to 16 readable bytes, `out` to 16 writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_siphash_2_4_128(
    data: *const u8,
    len: usize,
    key: *const u8,
    out: *mut u8,
) {
    unsafe {
        siphash_2_4_impl::<true>(data, len, key, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 16] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f,
    ];

    // Published SipHash-2-4 test vectors (input is 00 01 02 ...).
    const VECTORS: [u64; 16] = [
        0x726f_db47_dd0e_0e31,
        0x74f8_39c5_93dc_67fd,
        0x0d6c_8009_d9a9_4f5a,
        0x8567_6696_d7fb_7e2d,
        0xcf27_94e0_2771_87b7,
        0x1876_5564_cd99_a68d,
        0xcbc9_466e_58fe_e3ce,
        0xab02_00f5_8b01_d137,
        0x93f5_f579_9a93_2462,
        0x9e00_82df_0ba9_e4b0,
        0x7a5d_bbc5_94dd_b9f3,
        0xf4b3_2f46_226b_ada7,
        0x751e_8fbc_860e_e5fb,
        0x14ea_5627_c084_3d90,
        0xf723_ca90_8e7a_f2ee,
        0xa129_ca61_49be_45e5,
    ];

    fn hash64(data: &[u8]) -> u64 {
        let mut out = [0u8; 8];
        siphash_2_4(data, &KEY, &mut out);
        u64::from_le_bytes(out)
    }

    #[test]
    fn published_vectors() {
        let input: Vec<u8> = (0..16).collect();
        for (len, expected) in VECTORS.iter().enumerate() {
            assert_eq!(hash64(&input[..len]), *expected, "len={len}");
        }
    }

    #[test]
    fn long_input_is_chunked_correctly() {
        let input: Vec<u8> = (0..=255).collect();
        for len in 0..input.len() {
            let mut out = [0u8; 8];
            siphash_2_4(&input[..len], &KEY, &mut out);
        }
        assert_eq!(hash64(&input[..16]), 0x3f2a_cc7f_57c2_9bdb);
    }

    #[test]
    fn ffi_entry_points_match_core() {
        let input: Vec<u8> = (0..=200).collect();
        for len in 0..input.len() {
            let mut rust = [0u8; 16];
            let mut direct = [0u8; 16];
            siphash_2_4(&input[..len], &KEY, &mut direct);
            unsafe {
                llvm_siphash_2_4_128(input.as_ptr(), len, KEY.as_ptr(), rust.as_mut_ptr());
            }
            assert_eq!(rust, direct, "len={len}");
        }
    }
}

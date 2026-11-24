//===-- lib.rs - Rust port of LLVM's MD5 -------------------------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Bit-compatible reimplementation of the compression function and one-shot
// hash in llvm/lib/Support/MD5.cpp (RFC 1321).
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

#[inline(always)]
fn f(x: u32, y: u32, z: u32) -> u32 {
    z ^ (x & (y ^ z))
}

#[inline(always)]
fn g(x: u32, y: u32, z: u32) -> u32 {
    y ^ (z & (x ^ y))
}

#[inline(always)]
fn h(x: u32, y: u32, z: u32) -> u32 {
    x ^ y ^ z
}

#[inline(always)]
fn i(x: u32, y: u32, z: u32) -> u32 {
    y ^ (x | !z)
}

macro_rules! step {
    ($f:ident, $a:ident, $b:ident, $c:ident, $d:ident, $x:expr, $t:expr, $s:expr) => {
        $a = $a
            .wrapping_add($f($b, $c, $d))
            .wrapping_add($x)
            .wrapping_add($t)
            .rotate_left($s)
            .wrapping_add($b);
    };
}

#[inline(always)]
unsafe fn load32(p: *const u8) -> u32 {
    unsafe { u32::from_le(core::ptr::read_unaligned(p as *const u32)) }
}

const IV: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];

/// Runs the 64-step compression function over `blocks` full 64-byte blocks.
unsafe fn body_raw(state: &mut [u32; 4], data: *const u8, blocks: usize) {
    unsafe {
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut p = data;
        let mut left = blocks;

        while left > 0 {
            let (sa, sb, sc, sd) = (a, b, c, d);

            // Round 1
            step!(f, a, b, c, d, load32(p.add(0)), 0xd76aa478, 7);
            step!(f, d, a, b, c, load32(p.add(4)), 0xe8c7b756, 12);
            step!(f, c, d, a, b, load32(p.add(8)), 0x242070db, 17);
            step!(f, b, c, d, a, load32(p.add(12)), 0xc1bdceee, 22);
            step!(f, a, b, c, d, load32(p.add(16)), 0xf57c0faf, 7);
            step!(f, d, a, b, c, load32(p.add(20)), 0x4787c62a, 12);
            step!(f, c, d, a, b, load32(p.add(24)), 0xa8304613, 17);
            step!(f, b, c, d, a, load32(p.add(28)), 0xfd469501, 22);
            step!(f, a, b, c, d, load32(p.add(32)), 0x698098d8, 7);
            step!(f, d, a, b, c, load32(p.add(36)), 0x8b44f7af, 12);
            step!(f, c, d, a, b, load32(p.add(40)), 0xffff5bb1, 17);
            step!(f, b, c, d, a, load32(p.add(44)), 0x895cd7be, 22);
            step!(f, a, b, c, d, load32(p.add(48)), 0x6b901122, 7);
            step!(f, d, a, b, c, load32(p.add(52)), 0xfd987193, 12);
            step!(f, c, d, a, b, load32(p.add(56)), 0xa679438e, 17);
            step!(f, b, c, d, a, load32(p.add(60)), 0x49b40821, 22);
            // Round 2
            step!(g, a, b, c, d, load32(p.add(4)), 0xf61e2562, 5);
            step!(g, d, a, b, c, load32(p.add(24)), 0xc040b340, 9);
            step!(g, c, d, a, b, load32(p.add(44)), 0x265e5a51, 14);
            step!(g, b, c, d, a, load32(p.add(0)), 0xe9b6c7aa, 20);
            step!(g, a, b, c, d, load32(p.add(20)), 0xd62f105d, 5);
            step!(g, d, a, b, c, load32(p.add(40)), 0x02441453, 9);
            step!(g, c, d, a, b, load32(p.add(60)), 0xd8a1e681, 14);
            step!(g, b, c, d, a, load32(p.add(16)), 0xe7d3fbc8, 20);
            step!(g, a, b, c, d, load32(p.add(36)), 0x21e1cde6, 5);
            step!(g, d, a, b, c, load32(p.add(56)), 0xc33707d6, 9);
            step!(g, c, d, a, b, load32(p.add(12)), 0xf4d50d87, 14);
            step!(g, b, c, d, a, load32(p.add(32)), 0x455a14ed, 20);
            step!(g, a, b, c, d, load32(p.add(52)), 0xa9e3e905, 5);
            step!(g, d, a, b, c, load32(p.add(8)), 0xfcefa3f8, 9);
            step!(g, c, d, a, b, load32(p.add(28)), 0x676f02d9, 14);
            step!(g, b, c, d, a, load32(p.add(48)), 0x8d2a4c8a, 20);
            // Round 3
            step!(h, a, b, c, d, load32(p.add(20)), 0xfffa3942, 4);
            step!(h, d, a, b, c, load32(p.add(32)), 0x8771f681, 11);
            step!(h, c, d, a, b, load32(p.add(44)), 0x6d9d6122, 16);
            step!(h, b, c, d, a, load32(p.add(56)), 0xfde5380c, 23);
            step!(h, a, b, c, d, load32(p.add(4)), 0xa4beea44, 4);
            step!(h, d, a, b, c, load32(p.add(16)), 0x4bdecfa9, 11);
            step!(h, c, d, a, b, load32(p.add(28)), 0xf6bb4b60, 16);
            step!(h, b, c, d, a, load32(p.add(40)), 0xbebfbc70, 23);
            step!(h, a, b, c, d, load32(p.add(52)), 0x289b7ec6, 4);
            step!(h, d, a, b, c, load32(p.add(0)), 0xeaa127fa, 11);
            step!(h, c, d, a, b, load32(p.add(12)), 0xd4ef3085, 16);
            step!(h, b, c, d, a, load32(p.add(24)), 0x04881d05, 23);
            step!(h, a, b, c, d, load32(p.add(36)), 0xd9d4d039, 4);
            step!(h, d, a, b, c, load32(p.add(48)), 0xe6db99e5, 11);
            step!(h, c, d, a, b, load32(p.add(60)), 0x1fa27cf8, 16);
            step!(h, b, c, d, a, load32(p.add(8)), 0xc4ac5665, 23);
            // Round 4
            step!(i, a, b, c, d, load32(p.add(0)), 0xf4292244, 6);
            step!(i, d, a, b, c, load32(p.add(28)), 0x432aff97, 10);
            step!(i, c, d, a, b, load32(p.add(56)), 0xab9423a7, 15);
            step!(i, b, c, d, a, load32(p.add(20)), 0xfc93a039, 21);
            step!(i, a, b, c, d, load32(p.add(48)), 0x655b59c3, 6);
            step!(i, d, a, b, c, load32(p.add(12)), 0x8f0ccc92, 10);
            step!(i, c, d, a, b, load32(p.add(40)), 0xffeff47d, 15);
            step!(i, b, c, d, a, load32(p.add(4)), 0x85845dd1, 21);
            step!(i, a, b, c, d, load32(p.add(32)), 0x6fa87e4f, 6);
            step!(i, d, a, b, c, load32(p.add(60)), 0xfe2ce6e0, 10);
            step!(i, c, d, a, b, load32(p.add(24)), 0xa3014314, 15);
            step!(i, b, c, d, a, load32(p.add(52)), 0x4e0811a1, 21);
            step!(i, a, b, c, d, load32(p.add(16)), 0xf7537e82, 6);
            step!(i, d, a, b, c, load32(p.add(44)), 0xbd3af235, 10);
            step!(i, c, d, a, b, load32(p.add(8)), 0x2ad7d2bb, 15);
            step!(i, b, c, d, a, load32(p.add(36)), 0xeb86d391, 21);

            a = a.wrapping_add(sa);
            b = b.wrapping_add(sb);
            c = c.wrapping_add(sc);
            d = d.wrapping_add(sd);

            p = p.add(64);
            left -= 1;
        }

        state[0] = a;
        state[1] = b;
        state[2] = c;
        state[3] = d;
    }
}

unsafe fn hash_raw(data: *const u8, len: usize, out: *mut u8) {
    unsafe {
        let mut state = IV;
        let blocks = len / 64;
        body_raw(&mut state, data, blocks);

        let mut tail = [0u8; 128];
        let rem = len - blocks * 64;
        core::ptr::copy_nonoverlapping(data.add(blocks * 64), tail.as_mut_ptr(), rem);
        tail[rem] = 0x80;
        let padded = if rem < 56 { 64 } else { 128 };
        let bits = (len as u64).wrapping_mul(8);
        tail[padded - 8..padded].copy_from_slice(&bits.to_le_bytes());
        body_raw(&mut state, tail.as_ptr(), padded / 64);

        let mut k = 0;
        while k < 4 {
            core::ptr::copy_nonoverlapping(state[k].to_le_bytes().as_ptr(), out.add(k * 4), 4);
            k += 1;
        }
    }
}

/// Continues the MD5 state over `blocks` full 64-byte blocks. The caller keeps
/// the state and the buffered tail, as `llvm::MD5` does.
///
/// # Safety
/// `state` must point to four `u32` values and `data` to `blocks * 64` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_md5_body(state: *mut u32, data: *const u8, blocks: usize) {
    unsafe { body_raw(&mut *(state as *mut [u32; 4]), data, blocks) }
}

/// One-shot MD5 of `len` bytes into `out[0..16]`.
///
/// # Safety
/// `data` must be readable for `len` bytes (may be null when `len` is 0) and
/// `out` must be writable for 16 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_md5_hash(data: *const u8, len: usize, out: *mut u8) {
    unsafe { hash_raw(data, len, out) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(data: &[u8]) -> [u8; 16] {
        let mut out = [0u8; 16];
        unsafe { hash_raw(data.as_ptr(), data.len(), out.as_mut_ptr()) };
        out
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Loop-based RFC 1321 implementation used as the oracle.
    fn reference(data: &[u8]) -> [u8; 16] {
        const K: [u32; 64] = [
            0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
            0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
            0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
            0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
            0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
            0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
            0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
            0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
            0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
            0xeb86d391,
        ];
        const S: [u32; 64] = [
            7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20,
            5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
            6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
        ];

        let mut msg = data.to_vec();
        let bits = (data.len() as u64).wrapping_mul(8);
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&bits.to_le_bytes());

        let mut state = IV;
        for block in msg.chunks_exact(64) {
            let mut m = [0u32; 16];
            for (k, word) in m.iter_mut().enumerate() {
                *word = u32::from_le_bytes(block[k * 4..k * 4 + 4].try_into().unwrap());
            }
            let (mut a, mut b, mut c, mut d) = (state[0], state[1], state[2], state[3]);
            for step in 0..64 {
                let (fun, g) = match step / 16 {
                    0 => ((b & c) | (!b & d), step),
                    1 => ((d & b) | (!d & c), (5 * step + 1) % 16),
                    2 => (b ^ c ^ d, (3 * step + 5) % 16),
                    _ => (c ^ (b | !d), (7 * step) % 16),
                };
                let tmp = d;
                d = c;
                c = b;
                b = b.wrapping_add(
                    a.wrapping_add(fun)
                        .wrapping_add(K[step])
                        .wrapping_add(m[g])
                        .rotate_left(S[step]),
                );
                a = tmp;
            }
            state[0] = state[0].wrapping_add(a);
            state[1] = state[1].wrapping_add(b);
            state[2] = state[2].wrapping_add(c);
            state[3] = state[3].wrapping_add(d);
        }

        let mut out = [0u8; 16];
        for (k, word) in state.iter().enumerate() {
            out[k * 4..k * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        out
    }

    #[test]
    fn rfc1321_vectors() {
        let vectors: [(&[u8], &str); 7] = [
            (b"", "d41d8cd98f00b204e9800998ecf8427e"),
            (b"a", "0cc175b9c0f1b6a831c399e269772661"),
            (b"abc", "900150983cd24fb0d6963f7d28e17f72"),
            (b"message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                b"abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ];
        for (input, expected) in vectors {
            assert_eq!(hex(&digest(input)), expected);
        }
    }

    #[test]
    fn randomized_lengths() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let data: Vec<u8> = (0..4096).map(|_| next() as u8).collect();
        for len in 0..data.len() {
            assert_eq!(digest(&data[..len]), reference(&data[..len]), "len {len}");
        }
    }

    #[test]
    fn body_matches_hash_for_block_aligned_input() {
        let data: Vec<u8> = (0..256u16).map(|b| b as u8).collect();
        let mut state = IV;
        unsafe { body_raw(&mut state, data.as_ptr(), 4) };

        let mut reference_state = IV;
        let mut msg = data.clone();
        msg.extend_from_slice(&(256u64 * 8).to_le_bytes());
        // The reference below hashes exactly 4 blocks plus padding, so compare
        // the compression of the four data blocks directly.
        let mut msg_blocks = data.clone();
        msg_blocks.truncate(256);
        for block in msg_blocks.chunks_exact(64) {
            let mut m = [0u32; 16];
            for (k, word) in m.iter_mut().enumerate() {
                *word = u32::from_le_bytes(block[k * 4..k * 4 + 4].try_into().unwrap());
            }
            let (mut a, mut b, mut c, mut d) = (
                reference_state[0],
                reference_state[1],
                reference_state[2],
                reference_state[3],
            );
            for step in 0..64 {
                let (fun, g) = match step / 16 {
                    0 => ((b & c) | (!b & d), step),
                    1 => ((d & b) | (!d & c), (5 * step + 1) % 16),
                    2 => (b ^ c ^ d, (3 * step + 5) % 16),
                    _ => (c ^ (b | !d), (7 * step) % 16),
                };
                let tmp = d;
                d = c;
                c = b;
                b = b.wrapping_add(
                    a.wrapping_add(fun)
                        .wrapping_add(REF_K[step])
                        .wrapping_add(m[g])
                        .rotate_left(REF_S[step]),
                );
                a = tmp;
            }
            reference_state[0] = reference_state[0].wrapping_add(a);
            reference_state[1] = reference_state[1].wrapping_add(b);
            reference_state[2] = reference_state[2].wrapping_add(c);
            reference_state[3] = reference_state[3].wrapping_add(d);
        }
        assert_eq!(state, reference_state);
    }

    const REF_K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    const REF_S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
}

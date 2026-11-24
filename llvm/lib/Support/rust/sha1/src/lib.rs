//===-- lib.rs - Rust port of LLVM's SHA-1 -----------------------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Bit-compatible reimplementation of the compression function and one-shot
// hash in llvm/lib/Support/SHA1.cpp (FIPS 180-4).
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

const K0: u32 = 0x5a82_7999;
const K1: u32 = 0x6ed9_eba1;
const K2: u32 = 0x8f1b_bcdc;
const K3: u32 = 0xca62_c1d6;

const IV: [u32; 5] = [
    0x6745_2301,
    0xefcd_ab89,
    0x98ba_dcfe,
    0x1032_5476,
    0xc3d2_e1f0,
];

/// Expands the circular 16-word schedule: `w[i & 15] = rol(w[i] ^ ... , 1)`.
#[inline(always)]
fn blk(w: &mut [u32; 16], i: usize) -> u32 {
    let v = w[(i + 13) & 15] ^ w[(i + 8) & 15] ^ w[(i + 2) & 15] ^ w[i & 15];
    w[i & 15] = v.rotate_left(1);
    w[i & 15]
}

macro_rules! r0 {
    ($a:ident, $b:ident, $c:ident, $d:ident, $e:ident, $w:ident, $i:expr) => {{
        $e = $e
            .wrapping_add(($b & ($c ^ $d)) ^ $d)
            .wrapping_add($w[$i])
            .wrapping_add(K0)
            .wrapping_add($a.rotate_left(5));
        $b = $b.rotate_left(30);
    }};
}

macro_rules! r1 {
    ($a:ident, $b:ident, $c:ident, $d:ident, $e:ident, $w:ident, $i:expr) => {{
        $e = $e
            .wrapping_add(($b & ($c ^ $d)) ^ $d)
            .wrapping_add(blk(&mut $w, $i))
            .wrapping_add(K0)
            .wrapping_add($a.rotate_left(5));
        $b = $b.rotate_left(30);
    }};
}

macro_rules! r2 {
    ($a:ident, $b:ident, $c:ident, $d:ident, $e:ident, $w:ident, $i:expr) => {{
        $e = $e
            .wrapping_add($b ^ $c ^ $d)
            .wrapping_add(blk(&mut $w, $i))
            .wrapping_add(K1)
            .wrapping_add($a.rotate_left(5));
        $b = $b.rotate_left(30);
    }};
}

macro_rules! r3 {
    ($a:ident, $b:ident, $c:ident, $d:ident, $e:ident, $w:ident, $i:expr) => {{
        $e = $e
            .wrapping_add(($b & $c) | ($b & $d) | ($c & $d))
            .wrapping_add(blk(&mut $w, $i))
            .wrapping_add(K2)
            .wrapping_add($a.rotate_left(5));
        $b = $b.rotate_left(30);
    }};
}

macro_rules! r4 {
    ($a:ident, $b:ident, $c:ident, $d:ident, $e:ident, $w:ident, $i:expr) => {{
        $e = $e
            .wrapping_add($b ^ $c ^ $d)
            .wrapping_add(blk(&mut $w, $i))
            .wrapping_add(K3)
            .wrapping_add($a.rotate_left(5));
        $b = $b.rotate_left(30);
    }};
}

#[cfg(target_arch = "x86_64")]
static SHA_NI: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// Write-once cache of the SHA-NI feature check. `0` is unknown, `1` is
/// absent and `2` is present.
#[cfg(target_arch = "x86_64")]
fn has_sha_ni() -> bool {
    use core::sync::atomic::Ordering;
    match SHA_NI.load(Ordering::Relaxed) {
        2 => true,
        1 => false,
        _ => {
            let ok = {
                use core::arch::x86_64::{__cpuid, __cpuid_count};
                let leaf1 = __cpuid(1);
                let has_ssse3 = leaf1.ecx & (1 << 9) != 0;
                let has_sse41 = leaf1.ecx & (1 << 19) != 0;
                let leaf7 = __cpuid_count(7, 0);
                has_ssse3 && has_sse41 && leaf7.ebx & (1 << 29) != 0
            };
            SHA_NI.store(if ok { 2 } else { 1 }, Ordering::Relaxed);
            ok
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod ni {
    use core::arch::x86_64::*;

    macro_rules! rounds4 {
        ($h0:ident, $h1:ident, $wk:expr, $i:expr) => {
            _mm_sha1rnds4_epu32($h0, _mm_sha1nexte_epu32($h1, $wk), $i)
        };
    }

    macro_rules! schedule {
        ($v0:expr, $v1:expr, $v2:expr, $v3:expr) => {
            _mm_sha1msg2_epu32(_mm_xor_si128(_mm_sha1msg1_epu32($v0, $v1), $v2), $v3)
        };
    }

    macro_rules! schedule_rounds4 {
        ($h0:ident, $h1:ident, $w0:expr, $w1:expr, $w2:expr, $w3:expr, $w4:ident,
         $i:expr) => {{
            $w4 = schedule!($w0, $w1, $w2, $w3);
            $h1 = rounds4!($h0, $h1, $w4, $i);
        }};
    }

    #[target_feature(enable = "sha,sse2,ssse3,sse4.1")]
    #[allow(unsafe_op_in_unsafe_fn)]
    pub(super) unsafe fn compress(state: &mut [u32; 5], block: &[u32; 16]) {
        let mut abcd = _mm_shuffle_epi32(_mm_loadu_si128(state.as_ptr().cast()), 0b0001_1011);
        let mut e = _mm_set_epi32(state[4] as i32, 0, 0, 0);

        let mut w0 = _mm_set_epi32(
            block[0] as i32,
            block[1] as i32,
            block[2] as i32,
            block[3] as i32,
        );
        let mut w1 = _mm_set_epi32(
            block[4] as i32,
            block[5] as i32,
            block[6] as i32,
            block[7] as i32,
        );
        let mut w2 = _mm_set_epi32(
            block[8] as i32,
            block[9] as i32,
            block[10] as i32,
            block[11] as i32,
        );
        let mut w3 = _mm_set_epi32(
            block[12] as i32,
            block[13] as i32,
            block[14] as i32,
            block[15] as i32,
        );
        #[allow(clippy::needless_late_init)]
        let mut w4;

        let mut h0 = abcd;
        let mut h1 = _mm_add_epi32(e, w0);

        // Rounds 0..20
        h1 = _mm_sha1rnds4_epu32(h0, h1, 0);
        h0 = rounds4!(h1, h0, w1, 0);
        h1 = rounds4!(h0, h1, w2, 0);
        h0 = rounds4!(h1, h0, w3, 0);
        schedule_rounds4!(h0, h1, w0, w1, w2, w3, w4, 0);

        // Rounds 20..40
        schedule_rounds4!(h1, h0, w1, w2, w3, w4, w0, 1);
        schedule_rounds4!(h0, h1, w2, w3, w4, w0, w1, 1);
        schedule_rounds4!(h1, h0, w3, w4, w0, w1, w2, 1);
        schedule_rounds4!(h0, h1, w4, w0, w1, w2, w3, 1);
        schedule_rounds4!(h1, h0, w0, w1, w2, w3, w4, 1);

        // Rounds 40..60
        schedule_rounds4!(h0, h1, w1, w2, w3, w4, w0, 2);
        schedule_rounds4!(h1, h0, w2, w3, w4, w0, w1, 2);
        schedule_rounds4!(h0, h1, w3, w4, w0, w1, w2, 2);
        schedule_rounds4!(h1, h0, w4, w0, w1, w2, w3, 2);
        schedule_rounds4!(h0, h1, w0, w1, w2, w3, w4, 2);

        // Rounds 60..80
        schedule_rounds4!(h1, h0, w1, w2, w3, w4, w0, 3);
        schedule_rounds4!(h0, h1, w2, w3, w4, w0, w1, 3);
        schedule_rounds4!(h1, h0, w3, w4, w0, w1, w2, 3);
        schedule_rounds4!(h0, h1, w4, w0, w1, w2, w3, 3);
        schedule_rounds4!(h1, h0, w0, w1, w2, w3, w4, 3);

        abcd = _mm_add_epi32(abcd, h0);
        e = _mm_sha1nexte_epu32(h1, e);

        abcd = _mm_shuffle_epi32(abcd, 0b0001_1011);
        _mm_storeu_si128(state.as_mut_ptr().cast(), abcd);
        state[4] = _mm_extract_epi32(e, 3) as u32;
    }
}

/// Dispatches to the SHA-NI path when the CPU supports it.
fn compress(state: &mut [u32; 5], block: &[u32; 16]) {
    #[cfg(target_arch = "x86_64")]
    {
        if has_sha_ni() {
            unsafe { ni::compress(state, block) };
            return;
        }
    }
    compress_scalar(state, block);
}

fn compress_scalar(state: &mut [u32; 5], block: &[u32; 16]) {
    let mut w = *block;
    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];

    r0!(a, b, c, d, e, w, 0);
    r0!(e, a, b, c, d, w, 1);
    r0!(d, e, a, b, c, w, 2);
    r0!(c, d, e, a, b, w, 3);
    r0!(b, c, d, e, a, w, 4);
    r0!(a, b, c, d, e, w, 5);
    r0!(e, a, b, c, d, w, 6);
    r0!(d, e, a, b, c, w, 7);
    r0!(c, d, e, a, b, w, 8);
    r0!(b, c, d, e, a, w, 9);
    r0!(a, b, c, d, e, w, 10);
    r0!(e, a, b, c, d, w, 11);
    r0!(d, e, a, b, c, w, 12);
    r0!(c, d, e, a, b, w, 13);
    r0!(b, c, d, e, a, w, 14);
    r0!(a, b, c, d, e, w, 15);
    r1!(e, a, b, c, d, w, 16);
    r1!(d, e, a, b, c, w, 17);
    r1!(c, d, e, a, b, w, 18);
    r1!(b, c, d, e, a, w, 19);
    r2!(a, b, c, d, e, w, 20);
    r2!(e, a, b, c, d, w, 21);
    r2!(d, e, a, b, c, w, 22);
    r2!(c, d, e, a, b, w, 23);
    r2!(b, c, d, e, a, w, 24);
    r2!(a, b, c, d, e, w, 25);
    r2!(e, a, b, c, d, w, 26);
    r2!(d, e, a, b, c, w, 27);
    r2!(c, d, e, a, b, w, 28);
    r2!(b, c, d, e, a, w, 29);
    r2!(a, b, c, d, e, w, 30);
    r2!(e, a, b, c, d, w, 31);
    r2!(d, e, a, b, c, w, 32);
    r2!(c, d, e, a, b, w, 33);
    r2!(b, c, d, e, a, w, 34);
    r2!(a, b, c, d, e, w, 35);
    r2!(e, a, b, c, d, w, 36);
    r2!(d, e, a, b, c, w, 37);
    r2!(c, d, e, a, b, w, 38);
    r2!(b, c, d, e, a, w, 39);
    r3!(a, b, c, d, e, w, 40);
    r3!(e, a, b, c, d, w, 41);
    r3!(d, e, a, b, c, w, 42);
    r3!(c, d, e, a, b, w, 43);
    r3!(b, c, d, e, a, w, 44);
    r3!(a, b, c, d, e, w, 45);
    r3!(e, a, b, c, d, w, 46);
    r3!(d, e, a, b, c, w, 47);
    r3!(c, d, e, a, b, w, 48);
    r3!(b, c, d, e, a, w, 49);
    r3!(a, b, c, d, e, w, 50);
    r3!(e, a, b, c, d, w, 51);
    r3!(d, e, a, b, c, w, 52);
    r3!(c, d, e, a, b, w, 53);
    r3!(b, c, d, e, a, w, 54);
    r3!(a, b, c, d, e, w, 55);
    r3!(e, a, b, c, d, w, 56);
    r3!(d, e, a, b, c, w, 57);
    r3!(c, d, e, a, b, w, 58);
    r3!(b, c, d, e, a, w, 59);
    r4!(a, b, c, d, e, w, 60);
    r4!(e, a, b, c, d, w, 61);
    r4!(d, e, a, b, c, w, 62);
    r4!(c, d, e, a, b, w, 63);
    r4!(b, c, d, e, a, w, 64);
    r4!(a, b, c, d, e, w, 65);
    r4!(e, a, b, c, d, w, 66);
    r4!(d, e, a, b, c, w, 67);
    r4!(c, d, e, a, b, w, 68);
    r4!(b, c, d, e, a, w, 69);
    r4!(a, b, c, d, e, w, 70);
    r4!(e, a, b, c, d, w, 71);
    r4!(d, e, a, b, c, w, 72);
    r4!(c, d, e, a, b, w, 73);
    r4!(b, c, d, e, a, w, 74);
    r4!(a, b, c, d, e, w, 75);
    r4!(e, a, b, c, d, w, 76);
    r4!(d, e, a, b, c, w, 77);
    r4!(c, d, e, a, b, w, 78);
    r4!(b, c, d, e, a, w, 79);

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
}

#[inline(always)]
unsafe fn load_be32(p: *const u8) -> u32 {
    unsafe { u32::from_be(core::ptr::read_unaligned(p as *const u32)) }
}

/// Compresses `blocks` 64-byte big-endian blocks from `data`.
unsafe fn blocks_raw(state: &mut [u32; 5], data: *const u8, blocks: usize) {
    let mut w = [0u32; 16];
    let mut i = 0;
    while i < blocks {
        let p = unsafe { data.add(i * 64) };
        let mut k = 0;
        while k < 16 {
            w[k] = unsafe { load_be32(p.add(k * 4)) };
            k += 1;
        }
        compress(state, &w);
        i += 1;
    }
}

unsafe fn blocks_raw_scalar(state: &mut [u32; 5], data: *const u8, blocks: usize) {
    let mut w = [0u32; 16];
    let mut i = 0;
    while i < blocks {
        let p = unsafe { data.add(i * 64) };
        let mut k = 0;
        while k < 16 {
            w[k] = unsafe { load_be32(p.add(k * 4)) };
            k += 1;
        }
        compress_scalar(state, &w);
        i += 1;
    }
}

unsafe fn hash_raw_impl(data: *const u8, len: usize, out: *mut u8, scalar: bool) {
    let mut state = IV;
    let blocks = len / 64;
    let run = |state: &mut [u32; 5], p: *const u8, n: usize| unsafe {
        if scalar {
            blocks_raw_scalar(state, p, n)
        } else {
            blocks_raw(state, p, n)
        }
    };
    run(&mut state, data, blocks);

    let mut tail = [0u8; 128];
    let rem = len - blocks * 64;
    unsafe { core::ptr::copy_nonoverlapping(data.add(blocks * 64), tail.as_mut_ptr(), rem) };
    tail[rem] = 0x80;
    let padded = if rem < 56 { 64 } else { 128 };
    let bits = (len as u64).wrapping_mul(8);
    tail[padded - 8..padded].copy_from_slice(&bits.to_be_bytes());
    run(&mut state, tail.as_ptr(), padded / 64);

    let mut k = 0;
    while k < 5 {
        unsafe {
            core::ptr::copy_nonoverlapping(state[k].to_be_bytes().as_ptr(), out.add(k * 4), 4)
        };
        k += 1;
    }
}

/// Compresses one block whose 16 words are already in host byte order, which
/// is the layout of `SHA1::InternalState.Buffer.L`.
///
/// # Safety
/// `state` must point to five `u32` values and `block` to sixteen.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_sha1_block(state: *mut u32, block: *const u32) {
    unsafe {
        compress(
            &mut *(state as *mut [u32; 5]),
            &*(block as *const [u32; 16]),
        )
    }
}

/// Compresses `blocks` 64-byte big-endian blocks from `data`.
///
/// # Safety
/// `state` must point to five `u32` values and `data` to `blocks * 64` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_sha1_blocks(state: *mut u32, data: *const u8, blocks: usize) {
    unsafe { blocks_raw(&mut *(state as *mut [u32; 5]), data, blocks) }
}

/// One-shot SHA-1 of `len` bytes into `out[0..20]`.
///
/// # Safety
/// `data` must be readable for `len` bytes (may be null when `len` is 0) and
/// `out` must be writable for 20 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_sha1_hash(data: *const u8, len: usize, out: *mut u8) {
    unsafe { hash_raw_impl(data, len, out, false) }
}

/// One-shot hash that always uses the portable scalar path. Used by the
/// differential benchmark to separate the algorithm from the SHA-NI path.
///
/// # Safety
/// Same contract as `llvm_sha1_hash`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_sha1_hash_scalar(data: *const u8, len: usize, out: *mut u8) {
    unsafe { hash_raw_impl(data, len, out, true) }
}

/// Returns 1 when the SHA-NI path is available on the current CPU.
#[unsafe(no_mangle)]
pub extern "C" fn llvm_sha1_has_sha_ni() -> i32 {
    #[cfg(target_arch = "x86_64")]
    {
        if has_sha_ni() { 1 } else { 0 }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(data: &[u8]) -> [u8; 20] {
        let mut out = [0u8; 20];
        unsafe { hash_raw_impl(data.as_ptr(), data.len(), out.as_mut_ptr(), false) };
        out
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Straightforward FIPS 180-4 implementation used as the oracle.
    fn reference(data: &[u8]) -> [u8; 20] {
        let mut msg = data.to_vec();
        let bits = (data.len() as u64).wrapping_mul(8);
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&bits.to_be_bytes());

        let mut h: [u32; 5] = IV;
        for block in msg.chunks_exact(64) {
            let mut w = [0u32; 80];
            for k in 0..16 {
                w[k] = u32::from_be_bytes(block[k * 4..k * 4 + 4].try_into().unwrap());
            }
            for k in 16..80 {
                w[k] = (w[k - 3] ^ w[k - 8] ^ w[k - 14] ^ w[k - 16]).rotate_left(1);
            }
            let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
            for (k, word) in w.iter().enumerate() {
                let (f, kv) = match k / 20 {
                    0 => ((b & c) | (!b & d), K0),
                    1 => (b ^ c ^ d, K1),
                    2 => ((b & c) | (b & d) | (c & d), K2),
                    _ => (b ^ c ^ d, K3),
                };
                let t = a
                    .rotate_left(5)
                    .wrapping_add(f)
                    .wrapping_add(e)
                    .wrapping_add(kv)
                    .wrapping_add(*word);
                e = d;
                d = c;
                c = b.rotate_left(30);
                b = a;
                a = t;
            }
            h[0] = h[0].wrapping_add(a);
            h[1] = h[1].wrapping_add(b);
            h[2] = h[2].wrapping_add(c);
            h[3] = h[3].wrapping_add(d);
            h[4] = h[4].wrapping_add(e);
        }

        let mut out = [0u8; 20];
        for (k, word) in h.iter().enumerate() {
            out[k * 4..k * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    #[test]
    fn fips180_vectors() {
        let vectors: [(&[u8], &str); 4] = [
            (b"", "da39a3ee5e6b4b0d3255bfef95601890afd80709"),
            (b"abc", "a9993e364706816aba3e25717850c26c9cd0d89d"),
            (
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "84983e441c3bd26ebaae4aa1f95129e5e54670f1",
            ),
            (
                b"The quick brown fox jumps over the lazy dog",
                "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12",
            ),
        ];
        for (input, expected) in vectors {
            assert_eq!(hex(&digest(input)), expected);
        }
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            hex(&digest(&million)),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
    }

    #[test]
    fn sha_ni_matches_scalar() {
        #[cfg(target_arch = "x86_64")]
        {
            if !has_sha_ni() {
                return;
            }
            let mut seed = 0x0123_4567_89ab_cdefu64;
            for _ in 0..2000 {
                let mut block = [0u32; 16];
                for w in block.iter_mut() {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    *w = seed as u32;
                }
                let mut a = IV;
                let mut b = IV;
                unsafe { ni::compress(&mut a, &block) };
                compress_scalar(&mut b, &block);
                assert_eq!(a, b);
            }
        }
    }

    #[test]
    fn randomized_lengths() {
        let mut state = 0x243f_6a88_85a3_08d3u64;
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
}

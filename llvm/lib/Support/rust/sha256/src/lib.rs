//===-- lib.rs - Rust port of LLVM's SHA-256 ---------------------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Bit-compatible reimplementation of the compression function and one-shot
// hash in llvm/lib/Support/SHA256.cpp (FIPS 180-4).
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

const IV: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

#[inline(always)]
fn ch(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (!x & z)
}

#[inline(always)]
fn maj(x: u32, y: u32, z: u32) -> u32 {
    (x & y) ^ (x & z) ^ (y & z)
}

#[inline(always)]
fn sigma0(x: u32) -> u32 {
    x.rotate_right(2) ^ x.rotate_right(13) ^ x.rotate_right(22)
}

#[inline(always)]
fn sigma1(x: u32) -> u32 {
    x.rotate_right(6) ^ x.rotate_right(11) ^ x.rotate_right(25)
}

#[inline(always)]
fn sigma2(x: u32) -> u32 {
    x.rotate_right(17) ^ x.rotate_right(19) ^ (x >> 10)
}

#[inline(always)]
fn sigma3(x: u32) -> u32 {
    x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3)
}

/// One round of the compression function. `m1` is the message word being
/// consumed and is also replaced in place by the schedule word 16 steps ahead.
macro_rules! fexpand {
    ($a:ident, $b:ident, $c:ident, $d:ident, $e:ident, $f:ident, $g:ident, $h:ident,
     $m1:ident, $m2:ident, $m3:ident, $m4:ident, $k:expr) => {{
        $h = $h
            .wrapping_add(sigma1($e))
            .wrapping_add(ch($e, $f, $g))
            .wrapping_add($m1)
            .wrapping_add($k);
        $d = $d.wrapping_add($h);
        $h = $h.wrapping_add(sigma0($a)).wrapping_add(maj($a, $b, $c));
        $m1 = $m1
            .wrapping_add(sigma2($m2))
            .wrapping_add($m3)
            .wrapping_add(sigma3($m4));
    }};
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

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
            use core::arch::x86_64::{__cpuid, __cpuid_count};
            let leaf1 = __cpuid(1);
            let has_ssse3 = leaf1.ecx & (1 << 9) != 0;
            let has_sse41 = leaf1.ecx & (1 << 19) != 0;
            let leaf7 = __cpuid_count(7, 0);
            let ok = has_ssse3 && has_sse41 && leaf7.ebx & (1 << 29) != 0;
            SHA_NI.store(if ok { 2 } else { 1 }, Ordering::Relaxed);
            ok
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod ni {
    use super::K;
    use core::arch::x86_64::*;

    #[inline]
    #[target_feature(enable = "sha")]
    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn rounds4(r: usize, abef: &mut __m128i, cdgh: &mut __m128i, rest: __m128i) {
        let rk = _mm_set_epi32(
            K[4 * r + 3] as i32,
            K[4 * r + 2] as i32,
            K[4 * r + 1] as i32,
            K[4 * r] as i32,
        );
        let t1 = _mm_add_epi32(rest, rk);
        *cdgh = _mm_sha256rnds2_epu32(*cdgh, *abef, t1);
        let t2 = _mm_shuffle_epi32(t1, 0x0E);
        *abef = _mm_sha256rnds2_epu32(*abef, *cdgh, t2);
    }

    #[inline]
    #[target_feature(enable = "sha,ssse3")]
    #[allow(unsafe_op_in_unsafe_fn)]
    unsafe fn schedule_rounds16(
        r: usize,
        abef: &mut __m128i,
        cdgh: &mut __m128i,
        w: &mut [__m128i; 4],
    ) {
        for i in 0..4 {
            let w0 = w[i];
            let w1 = w[(i + 1) % 4];
            let w2 = w[(i + 2) % 4];
            let w3 = w[(i + 3) % 4];

            let t1 = _mm_sha256msg1_epu32(w0, w1);
            let t2 = _mm_alignr_epi8(w3, w2, 4);
            let t3 = _mm_add_epi32(t1, t2);

            w[i] = _mm_sha256msg2_epu32(t3, w3);
            rounds4(r + i, abef, cdgh, w[i]);
        }
    }

    #[target_feature(enable = "sha,sse2,ssse3,sse4.1")]
    #[allow(unsafe_op_in_unsafe_fn)]
    pub(super) unsafe fn compress(state: &mut [u32; 8], block: &[u32; 16]) {
        let dcba = _mm_loadu_si128(state.as_ptr().cast());
        let hgfe = _mm_loadu_si128(state.as_ptr().add(4).cast());
        let cdab = _mm_shuffle_epi32(dcba, 0xB1);
        let efgh = _mm_shuffle_epi32(hgfe, 0x1B);
        let mut abef = _mm_alignr_epi8(cdab, efgh, 8);
        let mut cdgh = _mm_blend_epi16(efgh, cdab, 0xF0);

        let abef_save = abef;
        let cdgh_save = cdgh;

        let mut w = [
            _mm_set_epi32(
                block[3] as i32,
                block[2] as i32,
                block[1] as i32,
                block[0] as i32,
            ),
            _mm_set_epi32(
                block[7] as i32,
                block[6] as i32,
                block[5] as i32,
                block[4] as i32,
            ),
            _mm_set_epi32(
                block[11] as i32,
                block[10] as i32,
                block[9] as i32,
                block[8] as i32,
            ),
            _mm_set_epi32(
                block[15] as i32,
                block[14] as i32,
                block[13] as i32,
                block[12] as i32,
            ),
        ];

        rounds4(0, &mut abef, &mut cdgh, w[0]);
        rounds4(1, &mut abef, &mut cdgh, w[1]);
        rounds4(2, &mut abef, &mut cdgh, w[2]);
        rounds4(3, &mut abef, &mut cdgh, w[3]);

        schedule_rounds16(4, &mut abef, &mut cdgh, &mut w);
        schedule_rounds16(8, &mut abef, &mut cdgh, &mut w);
        schedule_rounds16(12, &mut abef, &mut cdgh, &mut w);

        abef = _mm_add_epi32(abef, abef_save);
        cdgh = _mm_add_epi32(cdgh, cdgh_save);

        let feba = _mm_shuffle_epi32(abef, 0x1B);
        let dchg = _mm_shuffle_epi32(cdgh, 0xB1);
        let dcba = _mm_blend_epi16(feba, dchg, 0xF0);
        let hgfe = _mm_alignr_epi8(dchg, feba, 8);
        _mm_storeu_si128(state.as_mut_ptr().cast(), dcba);
        _mm_storeu_si128(state.as_mut_ptr().add(4).cast(), hgfe);
    }
}

/// Dispatches to the SHA-NI path when the CPU supports it.
fn compress(state: &mut [u32; 8], block: &[u32; 16]) {
    #[cfg(target_arch = "x86_64")]
    {
        if has_sha_ni() {
            unsafe { ni::compress(state, block) };
            return;
        }
    }
    compress_scalar(state, block);
}

#[allow(unused_assignments)]
fn compress_scalar(state: &mut [u32; 8], block: &[u32; 16]) {
    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];

    let mut w00 = block[0];
    let mut w01 = block[1];
    let mut w02 = block[2];
    let mut w03 = block[3];
    let mut w04 = block[4];
    let mut w05 = block[5];
    let mut w06 = block[6];
    let mut w07 = block[7];
    let mut w08 = block[8];
    let mut w09 = block[9];
    let mut w10 = block[10];
    let mut w11 = block[11];
    let mut w12 = block[12];
    let mut w13 = block[13];
    let mut w14 = block[14];
    let mut w15 = block[15];

    fexpand!(a, b, c, d, e, f, g, h, w00, w14, w09, w01, 0x428a2f98);
    fexpand!(h, a, b, c, d, e, f, g, w01, w15, w10, w02, 0x71374491);
    fexpand!(g, h, a, b, c, d, e, f, w02, w00, w11, w03, 0xb5c0fbcf);
    fexpand!(f, g, h, a, b, c, d, e, w03, w01, w12, w04, 0xe9b5dba5);
    fexpand!(e, f, g, h, a, b, c, d, w04, w02, w13, w05, 0x3956c25b);
    fexpand!(d, e, f, g, h, a, b, c, w05, w03, w14, w06, 0x59f111f1);
    fexpand!(c, d, e, f, g, h, a, b, w06, w04, w15, w07, 0x923f82a4);
    fexpand!(b, c, d, e, f, g, h, a, w07, w05, w00, w08, 0xab1c5ed5);
    fexpand!(a, b, c, d, e, f, g, h, w08, w06, w01, w09, 0xd807aa98);
    fexpand!(h, a, b, c, d, e, f, g, w09, w07, w02, w10, 0x12835b01);
    fexpand!(g, h, a, b, c, d, e, f, w10, w08, w03, w11, 0x243185be);
    fexpand!(f, g, h, a, b, c, d, e, w11, w09, w04, w12, 0x550c7dc3);
    fexpand!(e, f, g, h, a, b, c, d, w12, w10, w05, w13, 0x72be5d74);
    fexpand!(d, e, f, g, h, a, b, c, w13, w11, w06, w14, 0x80deb1fe);
    fexpand!(c, d, e, f, g, h, a, b, w14, w12, w07, w15, 0x9bdc06a7);
    fexpand!(b, c, d, e, f, g, h, a, w15, w13, w08, w00, 0xc19bf174);
    fexpand!(a, b, c, d, e, f, g, h, w00, w14, w09, w01, 0xe49b69c1);
    fexpand!(h, a, b, c, d, e, f, g, w01, w15, w10, w02, 0xefbe4786);
    fexpand!(g, h, a, b, c, d, e, f, w02, w00, w11, w03, 0x0fc19dc6);
    fexpand!(f, g, h, a, b, c, d, e, w03, w01, w12, w04, 0x240ca1cc);
    fexpand!(e, f, g, h, a, b, c, d, w04, w02, w13, w05, 0x2de92c6f);
    fexpand!(d, e, f, g, h, a, b, c, w05, w03, w14, w06, 0x4a7484aa);
    fexpand!(c, d, e, f, g, h, a, b, w06, w04, w15, w07, 0x5cb0a9dc);
    fexpand!(b, c, d, e, f, g, h, a, w07, w05, w00, w08, 0x76f988da);
    fexpand!(a, b, c, d, e, f, g, h, w08, w06, w01, w09, 0x983e5152);
    fexpand!(h, a, b, c, d, e, f, g, w09, w07, w02, w10, 0xa831c66d);
    fexpand!(g, h, a, b, c, d, e, f, w10, w08, w03, w11, 0xb00327c8);
    fexpand!(f, g, h, a, b, c, d, e, w11, w09, w04, w12, 0xbf597fc7);
    fexpand!(e, f, g, h, a, b, c, d, w12, w10, w05, w13, 0xc6e00bf3);
    fexpand!(d, e, f, g, h, a, b, c, w13, w11, w06, w14, 0xd5a79147);
    fexpand!(c, d, e, f, g, h, a, b, w14, w12, w07, w15, 0x06ca6351);
    fexpand!(b, c, d, e, f, g, h, a, w15, w13, w08, w00, 0x14292967);
    fexpand!(a, b, c, d, e, f, g, h, w00, w14, w09, w01, 0x27b70a85);
    fexpand!(h, a, b, c, d, e, f, g, w01, w15, w10, w02, 0x2e1b2138);
    fexpand!(g, h, a, b, c, d, e, f, w02, w00, w11, w03, 0x4d2c6dfc);
    fexpand!(f, g, h, a, b, c, d, e, w03, w01, w12, w04, 0x53380d13);
    fexpand!(e, f, g, h, a, b, c, d, w04, w02, w13, w05, 0x650a7354);
    fexpand!(d, e, f, g, h, a, b, c, w05, w03, w14, w06, 0x766a0abb);
    fexpand!(c, d, e, f, g, h, a, b, w06, w04, w15, w07, 0x81c2c92e);
    fexpand!(b, c, d, e, f, g, h, a, w07, w05, w00, w08, 0x92722c85);
    fexpand!(a, b, c, d, e, f, g, h, w08, w06, w01, w09, 0xa2bfe8a1);
    fexpand!(h, a, b, c, d, e, f, g, w09, w07, w02, w10, 0xa81a664b);
    fexpand!(g, h, a, b, c, d, e, f, w10, w08, w03, w11, 0xc24b8b70);
    fexpand!(f, g, h, a, b, c, d, e, w11, w09, w04, w12, 0xc76c51a3);
    fexpand!(e, f, g, h, a, b, c, d, w12, w10, w05, w13, 0xd192e819);
    fexpand!(d, e, f, g, h, a, b, c, w13, w11, w06, w14, 0xd6990624);
    fexpand!(c, d, e, f, g, h, a, b, w14, w12, w07, w15, 0xf40e3585);
    fexpand!(b, c, d, e, f, g, h, a, w15, w13, w08, w00, 0x106aa070);
    fexpand!(a, b, c, d, e, f, g, h, w00, w14, w09, w01, 0x19a4c116);
    fexpand!(h, a, b, c, d, e, f, g, w01, w15, w10, w02, 0x1e376c08);
    fexpand!(g, h, a, b, c, d, e, f, w02, w00, w11, w03, 0x2748774c);
    fexpand!(f, g, h, a, b, c, d, e, w03, w01, w12, w04, 0x34b0bcb5);
    fexpand!(e, f, g, h, a, b, c, d, w04, w02, w13, w05, 0x391c0cb3);
    fexpand!(d, e, f, g, h, a, b, c, w05, w03, w14, w06, 0x4ed8aa4a);
    fexpand!(c, d, e, f, g, h, a, b, w06, w04, w15, w07, 0x5b9cca4f);
    fexpand!(b, c, d, e, f, g, h, a, w07, w05, w00, w08, 0x682e6ff3);
    fexpand!(a, b, c, d, e, f, g, h, w08, w06, w01, w09, 0x748f82ee);
    fexpand!(h, a, b, c, d, e, f, g, w09, w07, w02, w10, 0x78a5636f);
    fexpand!(g, h, a, b, c, d, e, f, w10, w08, w03, w11, 0x84c87814);
    fexpand!(f, g, h, a, b, c, d, e, w11, w09, w04, w12, 0x8cc70208);
    fexpand!(e, f, g, h, a, b, c, d, w12, w10, w05, w13, 0x90befffa);
    fexpand!(d, e, f, g, h, a, b, c, w13, w11, w06, w14, 0xa4506ceb);
    fexpand!(c, d, e, f, g, h, a, b, w14, w12, w07, w15, 0xbef9a3f7);
    fexpand!(b, c, d, e, f, g, h, a, w15, w13, w08, w00, 0xc67178f2);

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

#[inline(always)]
unsafe fn load_be32(p: *const u8) -> u32 {
    unsafe { u32::from_be(core::ptr::read_unaligned(p as *const u32)) }
}

/// Compresses `blocks` 64-byte big-endian blocks from `data`.
unsafe fn blocks_raw(state: &mut [u32; 8], data: *const u8, blocks: usize) {
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

unsafe fn blocks_raw_scalar(state: &mut [u32; 8], data: *const u8, blocks: usize) {
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
    let run = |state: &mut [u32; 8], p: *const u8, n: usize| unsafe {
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
    while k < 8 {
        unsafe {
            core::ptr::copy_nonoverlapping(state[k].to_be_bytes().as_ptr(), out.add(k * 4), 4)
        };
        k += 1;
    }
}

/// Compresses one block whose 16 words are already in host byte order, which
/// is the layout of `SHA256::InternalState.Buffer.L`.
///
/// # Safety
/// `state` must point to eight `u32` values and `block` to sixteen.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_sha256_block(state: *mut u32, block: *const u32) {
    unsafe {
        compress(
            &mut *(state as *mut [u32; 8]),
            &*(block as *const [u32; 16]),
        )
    }
}

/// Compresses `blocks` 64-byte big-endian blocks from `data`.
///
/// # Safety
/// `state` must point to eight `u32` values and `data` to `blocks * 64` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_sha256_blocks(state: *mut u32, data: *const u8, blocks: usize) {
    unsafe { blocks_raw(&mut *(state as *mut [u32; 8]), data, blocks) }
}

/// One-shot SHA-256 of `len` bytes into `out[0..32]`.
///
/// # Safety
/// `data` must be readable for `len` bytes (may be null when `len` is 0) and
/// `out` must be writable for 32 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_sha256_hash(data: *const u8, len: usize, out: *mut u8) {
    unsafe { hash_raw_impl(data, len, out, false) }
}

/// One-shot hash that always uses the portable scalar path. Used by the
/// differential benchmark to separate the algorithm from the SHA-NI path.
///
/// # Safety
/// Same contract as `llvm_sha256_hash`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_sha256_hash_scalar(data: *const u8, len: usize, out: *mut u8) {
    unsafe { hash_raw_impl(data, len, out, true) }
}

/// Returns 1 when the SHA-NI path is available on the current CPU.
#[unsafe(no_mangle)]
pub extern "C" fn llvm_sha256_has_sha_ni() -> i32 {
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

    fn digest(data: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        unsafe { hash_raw_impl(data.as_ptr(), data.len(), out.as_mut_ptr(), false) };
        out
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Straightforward FIPS 180-4 implementation used as the oracle.
    fn reference(data: &[u8]) -> [u8; 32] {
        let mut msg = data.to_vec();
        let bits = (data.len() as u64).wrapping_mul(8);
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&bits.to_be_bytes());

        let mut h = IV;
        for block in msg.chunks_exact(64) {
            let mut w = [0u32; 64];
            for k in 0..16 {
                w[k] = u32::from_be_bytes(block[k * 4..k * 4 + 4].try_into().unwrap());
            }
            for k in 16..64 {
                w[k] = sigma2(w[k - 2])
                    .wrapping_add(w[k - 7])
                    .wrapping_add(sigma3(w[k - 15]))
                    .wrapping_add(w[k - 16]);
            }
            let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
                (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
            for k in 0..64 {
                let t1 = hh
                    .wrapping_add(sigma1(e))
                    .wrapping_add(ch(e, f, g))
                    .wrapping_add(K[k])
                    .wrapping_add(w[k]);
                let t2 = sigma0(a).wrapping_add(maj(a, b, c));
                hh = g;
                g = f;
                f = e;
                e = d.wrapping_add(t1);
                d = c;
                c = b;
                b = a;
                a = t1.wrapping_add(t2);
            }
            h[0] = h[0].wrapping_add(a);
            h[1] = h[1].wrapping_add(b);
            h[2] = h[2].wrapping_add(c);
            h[3] = h[3].wrapping_add(d);
            h[4] = h[4].wrapping_add(e);
            h[5] = h[5].wrapping_add(f);
            h[6] = h[6].wrapping_add(g);
            h[7] = h[7].wrapping_add(hh);
        }

        let mut out = [0u8; 32];
        for (k, word) in h.iter().enumerate() {
            out[k * 4..k * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    #[test]
    fn fips180_vectors() {
        let vectors: [(&[u8], &str); 4] = [
            (
                b"",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                b"abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
            (
                b"The quick brown fox jumps over the lazy dog",
                "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592",
            ),
        ];
        for (input, expected) in vectors {
            assert_eq!(hex(&digest(input)), expected);
        }
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            hex(&digest(&million)),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
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
        let mut state = 0x1319_8a2e_0370_7344u64;
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

//===-- lib.rs - Rust port of LLVM's Base64 ---------------------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Bit-compatible reimplementation of llvm/lib/Support/Base64.cpp and of the
// encodeBase64 template in llvm/include/llvm/Support/Base64.h.
//
// The encoder keeps the exact padding rules of the C++ template. On x86-64
// with SSSE3 it converts 12 input bytes into 16 output characters per
// iteration with a shuffle/multiply sequence instead of 16 table lookups,
// which is the part that makes it faster than the C++ loop. Everything else,
// including the tail and all non-SSSE3 targets, uses the same 3-byte scalar
// step as the C++ code.
//
// The decoder validates and converts four characters at a time and writes
// straight into the caller's buffer. It reproduces the C++ '=' placement rules
// and returns the index of the first invalid byte so the C++ wrapper can
// format the same error message.
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

const ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `=` maps to 0, which is what the C++ decoder's table does. Values outside
/// the alphabet are `0xFF`, a value no 6-bit digit can produce.
const fn make_decode_table(pad_valid: bool) -> [u8; 256] {
    let mut table = [0xFFu8; 256];
    let mut i = 0;
    while i < 26 {
        table[b'A' as usize + i] = i as u8;
        table[b'a' as usize + i] = 26 + i as u8;
        i += 1;
    }
    let mut i = 0;
    while i < 10 {
        table[b'0' as usize + i] = 52 + i as u8;
        i += 1;
    }
    table[b'+' as usize] = 62;
    table[b'/' as usize] = 63;
    if pad_valid {
        table[b'=' as usize] = 0;
    }
    table
}

/// Used for every chunk before the last one, where `=` is always illegal.
static DECODE_NO_PAD: [u8; 256] = make_decode_table(false);
/// Used for the last chunk, where `=` follows the C++ placement rules.
static DECODE: [u8; 256] = make_decode_table(true);

#[inline(always)]
unsafe fn write3(dst: *mut u8, a: u8, b: u8, c: u8, d: u8) {
    unsafe {
        *dst = (a << 2) | (b >> 4);
        *dst.add(1) = (b << 4) | (c >> 2);
        *dst.add(2) = (c << 6) | d;
    }
}

/// Scalar encoder: 3 bytes into 4 characters, identical to the C++ template.
unsafe fn encode_scalar(src: *const u8, len: usize, dst: *mut u8) {
    unsafe {
        let full = len / 3 * 3;
        let mut i = 0;
        let mut j = 0;
        while i < full {
            let x = ((*src.add(i) as u32) << 16)
                | ((*src.add(i + 1) as u32) << 8)
                | *src.add(i + 2) as u32;
            *dst.add(j) = ALPHABET[(x >> 18) as usize & 63];
            *dst.add(j + 1) = ALPHABET[(x >> 12) as usize & 63];
            *dst.add(j + 2) = ALPHABET[(x >> 6) as usize & 63];
            *dst.add(j + 3) = ALPHABET[x as usize & 63];
            i += 3;
            j += 4;
        }
        match len - full {
            1 => {
                let x = (*src.add(i) as u32) << 16;
                *dst.add(j) = ALPHABET[(x >> 18) as usize & 63];
                *dst.add(j + 1) = ALPHABET[(x >> 12) as usize & 63];
                *dst.add(j + 2) = b'=';
                *dst.add(j + 3) = b'=';
            }
            2 => {
                let x = ((*src.add(i) as u32) << 16) | ((*src.add(i + 1) as u32) << 8);
                *dst.add(j) = ALPHABET[(x >> 18) as usize & 63];
                *dst.add(j + 1) = ALPHABET[(x >> 12) as usize & 63];
                *dst.add(j + 2) = ALPHABET[(x >> 6) as usize & 63];
                *dst.add(j + 3) = b'=';
            }
            _ => {}
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod ssse3 {
    use core::sync::atomic::{AtomicU8, Ordering};

    static HAVE: AtomicU8 = AtomicU8::new(0);

    /// Write-once cache: `0` unknown, `1` absent, `2` present.
    pub(super) fn available() -> bool {
        match HAVE.load(Ordering::Relaxed) {
            2 => true,
            1 => false,
            _ => {
                let ok = core::arch::x86_64::__cpuid(1).ecx & (1 << 9) != 0;
                HAVE.store(if ok { 2 } else { 1 }, Ordering::Relaxed);
                ok
            }
        }
    }

    /// 12 bytes in, 16 characters out per iteration. Only called with
    /// `len` a multiple of 12 and at least 16 readable bytes, so the 16-byte
    /// load never crosses the end of the input.
    #[target_feature(enable = "ssse3")]
    #[allow(unsafe_op_in_unsafe_fn)]
    pub(super) unsafe fn encode(src: *const u8, dst: *mut u8, len: usize) {
        use core::arch::x86_64::*;

        // Spread each 3-byte group over a 32-bit lane.
        let spread = _mm_setr_epi8(1, 0, 2, 1, 4, 3, 5, 4, 7, 6, 8, 7, 10, 9, 11, 10);
        // Offsets that turn a 6-bit value into its ASCII digit. The index is
        // saturated at 51 and corrected by the "greater than 25" mask, so the
        // four ranges (A-Z, a-z, 0-9, +/) select entries 0/1/2..11/12/13.
        let translate = _mm_setr_epi8(
            65, 71, -4, -4, -4, -4, -4, -4, -4, -4, -4, -4, -19, -16, 0, 0,
        );
        let c51 = _mm_set1_epi8(51);
        let c25 = _mm_set1_epi8(25);
        let m0 = _mm_set1_epi32(0x0fc0_fc00u32 as i32);
        let m1 = _mm_set1_epi32(0x0400_0040u32 as i32);
        let m2 = _mm_set1_epi32(0x003f_03f0u32 as i32);
        let m3 = _mm_set1_epi32(0x0100_0010u32 as i32);

        let mut i = 0;
        while i < len {
            let v = _mm_loadu_si128(src.add(i).cast());
            let v = _mm_shuffle_epi8(v, spread);
            let hi = _mm_mulhi_epu16(_mm_and_si128(v, m0), m1);
            let lo = _mm_mullo_epi16(_mm_and_si128(v, m2), m3);
            let v = _mm_or_si128(hi, lo);
            let idx = _mm_subs_epu8(v, c51);
            let idx = _mm_sub_epi8(idx, _mm_cmpgt_epi8(v, c25));
            let v = _mm_add_epi8(v, _mm_shuffle_epi8(translate, idx));
            _mm_storeu_si128(dst.add(i / 3 * 4).cast(), v);
            i += 12;
        }
    }
}

/// Encodes `len` bytes from `src` into `dst`. `dst` must have room for
/// `(len + 2) / 3 * 4` bytes. Returns the number of bytes written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_base64_encode(
    src: *const u8,
    len: usize,
    dst: *mut u8,
) -> usize {
    if len == 0 {
        return 0;
    }
    #[cfg(target_arch = "x86_64")]
    {
        if ssse3::available() {
            // The vector load reads 16 bytes, so only start a block when the
            // 4 bytes past it still belong to the input.
            let vec_len = if len >= 4 { (len - 4) / 12 * 12 } else { 0 };
            unsafe {
                if vec_len != 0 {
                    ssse3::encode(src, dst, vec_len);
                }
                encode_scalar(src.add(vec_len), len - vec_len, dst.add(vec_len / 3 * 4));
            }
            return len.div_ceil(3) * 4;
        }
    }
    unsafe { encode_scalar(src, len, dst) };
    len.div_ceil(3) * 4
}

/// Decodes `len` bytes from `src` into `dst`. `len` must be a multiple of 4
/// and `dst` must have room for `len / 4 * 3` bytes.
///
/// Returns 0 on success, 1 if `len` is not a multiple of 4, and 2 for an
/// invalid character. In both error cases `*out_len` is the number of bytes
/// written before the error and `*bad_index` identifies the offending byte,
/// matching the partially filled vector the C++ decoder leaves behind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_base64_decode(
    src: *const u8,
    len: usize,
    dst: *mut u8,
    out_len: *mut usize,
    bad_index: *mut usize,
) -> i32 {
    unsafe {
        *out_len = 0;
        if len == 0 {
            return 0;
        }
        if len % 4 != 0 {
            return 1;
        }
        let chunks = len / 4;
        let mut written = 0;
        let mut chunk = 0;
        while chunk + 1 < chunks {
            let i = chunk * 4;
            let a = *DECODE_NO_PAD.get_unchecked(*src.add(i) as usize);
            let b = *DECODE_NO_PAD.get_unchecked(*src.add(i + 1) as usize);
            let c = *DECODE_NO_PAD.get_unchecked(*src.add(i + 2) as usize);
            let d = *DECODE_NO_PAD.get_unchecked(*src.add(i + 3) as usize);
            if (a | b | c | d) & 0x80 != 0 {
                let bad = if a & 0x80 != 0 {
                    i
                } else if b & 0x80 != 0 {
                    i + 1
                } else if c & 0x80 != 0 {
                    i + 2
                } else {
                    i + 3
                };
                *bad_index = bad;
                *out_len = written;
                return 2;
            }
            write3(dst.add(written), a, b, c, d);
            written += 3;
            chunk += 1;
        }

        let i = (chunks - 1) * 4;
        let mut values = [0u8; 4];
        let mut k = 0;
        while k < 4 {
            let byte = *src.add(i + k);
            let decoded = *DECODE.get_unchecked(byte as usize);
            let mut illegal = decoded == 0xFF;
            if !illegal && byte == b'=' {
                if i + k < len - 2 {
                    illegal = true;
                } else if i + k == len - 2 && *src.add(i + k + 1) != b'=' {
                    illegal = true;
                }
            }
            if illegal {
                *bad_index = i + k;
                *out_len = written;
                return 2;
            }
            values[k] = decoded;
            k += 1;
        }
        write3(dst.add(written), values[0], values[1], values[2], values[3]);
        written += 3;
        if *src.add(len - 1) == b'=' {
            written -= 1;
            if *src.add(len - 2) == b'=' {
                written -= 1;
            }
        }
        *out_len = written;
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; data.len().div_ceil(3) * 4];
        let n = unsafe { llvm_base64_encode(data.as_ptr(), data.len(), out.as_mut_ptr()) };
        out.truncate(n);
        out
    }

    fn decode(data: &[u8]) -> Result<Vec<u8>, usize> {
        let mut out = std::vec![0u8; data.len() / 4 * 3];
        let mut n = 0;
        let mut bad = 0;
        let rc = unsafe {
            llvm_base64_decode(
                data.as_ptr(),
                data.len(),
                out.as_mut_ptr(),
                &mut n,
                &mut bad,
            )
        };
        out.truncate(n);
        match rc {
            0 => Ok(out),
            _ => Err(bad),
        }
    }

    #[test]
    fn rfc4648_vectors() {
        let cases: &[(&[u8], &[u8])] = &[
            (b"", b""),
            (b"f", b"Zg=="),
            (b"fo", b"Zm8="),
            (b"foo", b"Zm9v"),
            (b"foob", b"Zm9vYg=="),
            (b"fooba", b"Zm9vYmE="),
            (b"foobar", b"Zm9vYmFy"),
        ];
        for (plain, encoded) in cases {
            assert_eq!(encode(plain), *encoded);
            assert_eq!(decode(encoded).unwrap(), *plain);
        }
    }

    #[test]
    fn lengths_match_reference_formula() {
        let data: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
        for len in 0..=600 {
            let out = encode(&data[..len]);
            assert_eq!(out.len(), len.div_ceil(3) * 4);
            assert_eq!(decode(&out).unwrap(), &data[..len]);
        }
    }

    #[test]
    fn padding_placement_rules() {
        // Only the last two characters may be '='; the second-to-last only
        // when the last is also '='.
        assert!(decode(b"AA==").is_ok());
        assert!(decode(b"AAA=").is_ok());
        assert_eq!(decode(b"A==="), Err(1));
        assert_eq!(decode(b"=AAA"), Err(0));
        assert_eq!(decode(b"AA=A"), Err(2));
        assert_eq!(decode(b"===="), Err(0));
    }

    #[test]
    fn invalid_bytes_report_first_index() {
        for pos in 0..4 {
            for byte in 0..=255u8 {
                if byte.is_ascii_alphanumeric() || byte == b'+' || byte == b'/' {
                    continue;
                }
                let mut input = *b"AAAA";
                input[pos] = byte;
                let expected = if byte == b'=' {
                    if pos == 3 {
                        None
                    } else if pos == 2 {
                        // "AA=A" has '=' at index 2 with a non-'=' after it.
                        Some(pos)
                    } else {
                        Some(pos)
                    }
                } else {
                    Some(pos)
                };
                match (decode(&input), expected) {
                    (Ok(_), None) => {}
                    (Err(i), Some(p)) if i == p => {}
                    (got, want) => panic!("input {input:?}: got {got:?}, want {want:?}"),
                }
            }
        }
    }
}

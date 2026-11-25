//===-- lib.rs - Rust port of LLVM's DJB hash -----------------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Bit-compatible port of llvm/lib/Support/DJB.cpp. The ASCII path is kept
// separate so common debug-info names avoid UTF-8 decoding entirely.
//
//===----------------------------------------------------------------------===//

#![cfg_attr(not(test), no_std)]
#![allow(clippy::missing_safety_doc)]
#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    unsafe extern "C" {
        fn abort() -> !;
    }
    unsafe { abort() }
}

const UNI_REPLACEMENT_CHAR: u32 = 0x0000_FFFD;
const UNI_MAX_LEGAL_UTF32: u32 = 0x0010_FFFF;
const UNI_SUR_HIGH_START: u32 = 0xD800;
const UNI_SUR_LOW_END: u32 = 0xDFFF;

const TRAILING_BYTES: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0xC0;
    while i <= 0xDF {
        t[i] = 1;
        i += 1;
    }
    while i <= 0xEF {
        t[i] = 2;
        i += 1;
    }
    while i <= 0xF7 {
        t[i] = 3;
        i += 1;
    }
    while i <= 0xFB {
        t[i] = 4;
        i += 1;
    }
    while i <= 0xFF {
        t[i] = 5;
        i += 1;
    }
    t
};

const OFFSETS: [u32; 6] = [
    0x0000_0000,
    0x0000_3080,
    0x000E_2080,
    0x03C8_2080,
    0xFA08_2080,
    0x8208_2080,
];

const ASCII_FOLD: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        table[i] = if i >= b'A' as usize && i <= b'Z' as usize {
            (i + 32) as u8
        } else {
            i as u8
        };
        i += 1;
    }
    table
};

#[inline(always)]
fn is_continuation(b: u8) -> bool {
    (0x80..=0xBF).contains(&b)
}

#[inline(always)]
fn inner_switch(first: u8, a: u8) -> bool {
    match first {
        0xE0 => a >= 0xA0,
        0xED => a <= 0x9F,
        0xF0 => a >= 0x90,
        0xF4 => a <= 0x8F,
        _ => true,
    }
}

#[inline(always)]
unsafe fn is_legal_utf8(source: *const u8, length: usize) -> bool {
    let first = unsafe { *source };
    match length {
        1 => !(0x80..0xC2).contains(&first) && first <= 0xF4,
        2 => {
            let b1 = unsafe { *source.add(1) };
            is_continuation(b1)
                && inner_switch(first, b1)
                && !(0x80..0xC2).contains(&first)
                && first <= 0xF4
        }
        3 => {
            let b2 = unsafe { *source.add(2) };
            let b1 = unsafe { *source.add(1) };
            is_continuation(b2)
                && is_continuation(b1)
                && inner_switch(first, b1)
                && !(0x80..0xC2).contains(&first)
                && first <= 0xF4
        }
        4 => {
            let b3 = unsafe { *source.add(3) };
            let b2 = unsafe { *source.add(2) };
            let b1 = unsafe { *source.add(1) };
            is_continuation(b3)
                && is_continuation(b2)
                && is_continuation(b1)
                && inner_switch(first, b1)
                && !(0x80..0xC2).contains(&first)
                && first <= 0xF4
        }
        _ => false,
    }
}

#[inline(always)]
unsafe fn find_maximal_subpart(source: *const u8, source_end: *const u8) -> u32 {
    if source == source_end {
        return 0;
    }
    let b1 = unsafe { *source };
    let mut p = unsafe { source.add(1) };
    if (0xC2..=0xDF).contains(&b1) {
        return 1;
    }
    if p == source_end {
        return 1;
    }
    let b2 = unsafe { *p };
    p = unsafe { p.add(1) };
    if b1 == 0xE0 {
        return if (0xA0..=0xBF).contains(&b2) { 2 } else { 1 };
    }
    if (0xE1..=0xEC).contains(&b1) {
        return if (0x80..=0xBF).contains(&b2) { 2 } else { 1 };
    }
    if b1 == 0xED {
        return if (0x80..=0x9F).contains(&b2) { 2 } else { 1 };
    }
    if (0xEE..=0xEF).contains(&b1) {
        return if (0x80..=0xBF).contains(&b2) { 2 } else { 1 };
    }
    if b1 == 0xF0 {
        if (0x90..=0xBF).contains(&b2) {
            if p == source_end {
                return 2;
            }
            let b3 = unsafe { *p };
            return if (0x80..=0xBF).contains(&b3) { 3 } else { 2 };
        }
        return 1;
    }
    if (0xF1..=0xF3).contains(&b1) {
        if (0x80..=0xBF).contains(&b2) {
            if p == source_end {
                return 2;
            }
            let b3 = unsafe { *p };
            return if (0x80..=0xBF).contains(&b3) { 3 } else { 2 };
        }
        return 1;
    }
    if b1 == 0xF4 {
        if (0x80..=0x8F).contains(&b2) {
            if p == source_end {
                return 2;
            }
            let b3 = unsafe { *p };
            return if (0x80..=0xBF).contains(&b3) { 3 } else { 2 };
        }
        return 1;
    }
    1
}

#[inline(always)]
unsafe fn decode_utf8(source: *const u8, extra: usize) -> u32 {
    match extra {
        0 => (unsafe { *source }) as u32,
        1 => ((unsafe { *source } as u32) << 6)
            .wrapping_add(unsafe { *source.add(1) } as u32)
            .wrapping_sub(OFFSETS[1]),
        2 => ((unsafe { *source } as u32) << 12)
            .wrapping_add((unsafe { *source.add(1) } as u32) << 6)
            .wrapping_add(unsafe { *source.add(2) } as u32)
            .wrapping_sub(OFFSETS[2]),
        3 => ((unsafe { *source } as u32) << 18)
            .wrapping_add((unsafe { *source.add(1) } as u32) << 12)
            .wrapping_add((unsafe { *source.add(2) } as u32) << 6)
            .wrapping_add(unsafe { *source.add(3) } as u32)
            .wrapping_sub(OFFSETS[3]),
        _ => {
            let mut ch = 0u32;
            let mut i = 0usize;
            while i <= extra {
                ch = ch.wrapping_add(unsafe { *source.add(i) } as u32);
                if i < extra {
                    ch = ch.wrapping_shl(6);
                }
                i += 1;
            }
            ch.wrapping_sub(OFFSETS[extra])
        }
    }
}

#[inline(always)]
fn write_utf8(ch: u32, out: &mut [u8; 4]) -> usize {
    if ch < 0x80 {
        out[0] = ch as u8;
        1
    } else if ch < 0x800 {
        out[0] = (0xC0 | (ch >> 6)) as u8;
        out[1] = (0x80 | (ch & 0x3F)) as u8;
        2
    } else if ch < 0x10000 {
        out[0] = (0xE0 | (ch >> 12)) as u8;
        out[1] = (0x80 | ((ch >> 6) & 0x3F)) as u8;
        out[2] = (0x80 | (ch & 0x3F)) as u8;
        3
    } else {
        out[0] = (0xF0 | (ch >> 18)) as u8;
        out[1] = (0x80 | ((ch >> 12) & 0x3F)) as u8;
        out[2] = (0x80 | ((ch >> 6) & 0x3F)) as u8;
        out[3] = (0x80 | (ch & 0x3F)) as u8;
        4
    }
}

/// Mirrors `chopOneUTF32` in lenient mode. Returns the code point and the
/// number of bytes consumed.
#[inline(always)]
fn chop_one_utf32(input: &[u8]) -> (u32, usize) {
    if input.is_empty() {
        return (0, 0);
    }
    let first = input[0];
    if first < 0x80 {
        return (first as u32, 1);
    }
    let extra = TRAILING_BYTES[first as usize] as usize;
    if extra >= input.len() {
        let n = unsafe { find_maximal_subpart(input.as_ptr(), input.as_ptr().add(input.len())) }
            as usize;
        return (UNI_REPLACEMENT_CHAR, n.max(1));
    }
    if !unsafe { is_legal_utf8(input.as_ptr(), extra + 1) } {
        let n = unsafe { find_maximal_subpart(input.as_ptr(), input.as_ptr().add(input.len())) }
            as usize;
        return (UNI_REPLACEMENT_CHAR, n.max(1));
    }
    let ch = unsafe { decode_utf8(input.as_ptr(), extra) };
    if ch > UNI_MAX_LEGAL_UTF32 || (UNI_SUR_HIGH_START..=UNI_SUR_LOW_END).contains(&ch) {
        return (UNI_REPLACEMENT_CHAR, extra + 1);
    }
    (ch, extra + 1)
}

#[inline(always)]
fn djb_hash(bytes: &[u8], mut h: u32) -> u32 {
    for &byte in bytes {
        h = h.wrapping_mul(33).wrapping_add(byte as u32);
    }
    h
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_djb_case_folding_hash(data: *const u8, len: usize, h: u32) -> u32 {
    let input = if len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, len) }
    };

    let mut hash = h;
    let mut all_ascii = true;
    for &byte in input {
        hash = (hash << 5)
            .wrapping_add(hash)
            .wrapping_add(ASCII_FOLD[byte as usize] as u32);
        all_ascii &= byte < 0x80;
    }
    if all_ascii {
        return hash;
    }

    hash = h;
    let mut pos = 0usize;
    let mut storage = [0u8; 4];
    while pos < input.len() {
        let (mut ch, consumed) = chop_one_utf32(&input[pos..]);
        pos += consumed;
        if ch == 0x130 || ch == 0x131 {
            ch = b'i' as u32;
        } else {
            ch = fold_char_simple(ch as i32) as u32;
        }
        let n = write_utf8(ch, &mut storage);
        hash = djb_hash(&storage[..n], hash);
    }
    hash
}

include!("fold.rs");

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(bytes: &[u8]) -> u32 {
        unsafe { llvm_djb_case_folding_hash(bytes.as_ptr(), bytes.len(), 5381) }
    }

    #[test]
    fn ascii_hashes() {
        assert_eq!(hash(b""), 5381);
        assert_eq!(hash(b"a"), 177670);
        assert_eq!(hash(b"Hello"), hash(b"hello"));
        assert_eq!(hash(b"ABC"), hash(b"abc"));
    }

    #[test]
    fn dwarf_dotless_i() {
        assert_eq!(hash("\u{130}".as_bytes()), hash(b"i"));
        assert_eq!(hash("\u{131}".as_bytes()), hash(b"i"));
    }

    #[test]
    fn unicode_folding() {
        assert_eq!(hash("\u{00c4}".as_bytes()), hash("\u{00e4}".as_bytes()));
        assert_eq!(hash("\u{0391}".as_bytes()), hash("\u{03b1}".as_bytes()));
    }

    #[test]
    fn invalid_utf8_is_replacement() {
        assert_eq!(hash(&[0xff]), hash("\u{fffd}".as_bytes()));
    }

    #[test]
    fn fold_table_matches_identity_below_capital_a() {
        for c in 0..0x41 {
            assert_eq!(fold_char_simple(c), c);
        }
    }
}

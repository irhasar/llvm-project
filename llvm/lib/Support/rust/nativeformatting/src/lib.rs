//===-- lib.rs - Rust port of NativeFormatting -------------------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Bit-compatible decimal and hexadecimal digit formatting for
// llvm/lib/Support/NativeFormatting.cpp. The C++ wrapper keeps the raw_ostream
// and sign/min-width policy; this crate only turns a magnitude into digits.
//
//===----------------------------------------------------------------------===//

#![cfg_attr(not(test), no_std)]
#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    unsafe extern "C" {
        fn abort() -> !;
    }
    unsafe { abort() }
}

const MAX_DECIMAL_DIGITS: usize = 20;

const fn make_digit_pairs() -> [u8; 200] {
    let mut table = [0u8; 200];
    let mut i = 0usize;
    while i < 100 {
        table[i * 2] = b'0' + (i / 10) as u8;
        table[i * 2 + 1] = b'0' + (i % 10) as u8;
        i += 1;
    }
    table
}

const DIGIT_PAIRS: [u8; 200] = make_digit_pairs();

const fn make_hex_pairs(upper: bool) -> [u8; 512] {
    let mut table = [0u8; 512];
    let digits = if upper {
        *b"0123456789ABCDEF"
    } else {
        *b"0123456789abcdef"
    };
    let mut i = 0usize;
    while i < 256 {
        table[i * 2] = digits[i >> 4];
        table[i * 2 + 1] = digits[i & 15];
        i += 1;
    }
    table
}

const HEX_PAIRS_LOWER: [u8; 512] = make_hex_pairs(false);
const HEX_PAIRS_UPPER: [u8; 512] = make_hex_pairs(true);

#[inline(always)]
unsafe fn write_hex2(table: &[u8; 512], dst: *mut u8, byte: usize) {
    *dst = *table.get_unchecked(byte * 2);
    *dst.add(1) = *table.get_unchecked(byte * 2 + 1);
}

#[inline(always)]
unsafe fn write_hex4(table: &[u8; 512], dst: *mut u8, word: usize) {
    write_hex2(table, dst, (word >> 8) & 0xFF);
    write_hex2(table, dst.add(2), word & 0xFF);
}

#[inline(always)]
unsafe fn write_hex8(table: &[u8; 512], dst: *mut u8, word: u32) {
    write_hex4(table, dst, (word >> 16) as usize);
    write_hex4(table, dst.add(4), (word & 0xFFFF) as usize);
}

#[inline]
fn digits_u32(mut value: u32, buf: &mut [u8; MAX_DECIMAL_DIGITS]) -> usize {
    let mut pos = MAX_DECIMAL_DIGITS;
    while value >= 100 {
        let rem = (value % 100) as usize;
        value /= 100;
        pos -= 2;
        buf[pos] = DIGIT_PAIRS[rem * 2];
        buf[pos + 1] = DIGIT_PAIRS[rem * 2 + 1];
    }
    if value < 10 {
        pos -= 1;
        buf[pos] = b'0' + value as u8;
    } else {
        let rem = value as usize;
        pos -= 2;
        buf[pos] = DIGIT_PAIRS[rem * 2];
        buf[pos + 1] = DIGIT_PAIRS[rem * 2 + 1];
    }
    MAX_DECIMAL_DIGITS - pos
}

#[inline]
fn digits_u64(mut value: u64, buf: &mut [u8; MAX_DECIMAL_DIGITS]) -> usize {
    let mut pos = MAX_DECIMAL_DIGITS;
    while value >= 100 {
        let rem = (value % 100) as usize;
        value /= 100;
        pos -= 2;
        buf[pos] = DIGIT_PAIRS[rem * 2];
        buf[pos + 1] = DIGIT_PAIRS[rem * 2 + 1];
    }
    if value < 10 {
        pos -= 1;
        buf[pos] = b'0' + value as u8;
    } else {
        let rem = value as usize;
        pos -= 2;
        buf[pos] = DIGIT_PAIRS[rem * 2];
        buf[pos + 1] = DIGIT_PAIRS[rem * 2 + 1];
    }
    MAX_DECIMAL_DIGITS - pos
}

/// Writes decimal digits, optionally inserting the same thousands separators
/// as `writeWithCommas` in NativeFormatting.cpp. The sign and minimum-width
/// padding are intentionally left to the C++ caller.
///
/// Returns the number of bytes required. If `cap` is too small, nothing is
/// written and the required size is still returned.
///
/// # Safety
///
/// `out` must be writable for at least `cap` bytes when `cap` is non-zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_nativeformatting_format_decimal(
    value: u64,
    grouping: bool,
    out: *mut u8,
    cap: usize,
) -> usize {
    let mut digits = [0u8; MAX_DECIMAL_DIGITS];
    let len = if value <= u32::MAX as u64 {
        digits_u32(value as u32, &mut digits)
    } else {
        digits_u64(value, &mut digits)
    };
    let start = MAX_DECIMAL_DIGITS - len;

    let commas = if grouping && len > 3 {
        (len - 1) / 3
    } else {
        0
    };
    let total = len + commas;
    if cap < total {
        return total;
    }
    if total == 0 {
        return 0;
    }

    let dst = core::slice::from_raw_parts_mut(out, total);
    if !grouping || commas == 0 {
        dst.copy_from_slice(&digits[start..]);
        return total;
    }

    let initial = (len - 1) % 3 + 1;
    let mut src = start;
    let mut pos = 0usize;
    dst[pos..pos + initial].copy_from_slice(&digits[src..src + initial]);
    src += initial;
    pos += initial;
    while src < MAX_DECIMAL_DIGITS {
        dst[pos] = b',';
        pos += 1;
        dst[pos..pos + 3].copy_from_slice(&digits[src..src + 3]);
        src += 3;
        pos += 3;
    }
    total
}

/// Writes a hexadecimal number using the same width and prefix rules as
/// `llvm::write_hex`. `width` is clamped to 128 by the caller, matching the
/// C++ implementation.
///
/// Returns the number of bytes required. If `cap` is too small, nothing is
/// written and the required size is still returned.
///
/// # Safety
///
/// `out` must be writable for at least `cap` bytes when `cap` is non-zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_nativeformatting_format_hex(
    value: u64,
    upper: bool,
    prefix: bool,
    width: usize,
    out: *mut u8,
    cap: usize,
) -> usize {
    let width = width.min(128);
    let nibbles = if value == 0 {
        0
    } else {
        (64 - value.leading_zeros() as usize).div_ceil(4)
    };
    let prefix_chars = if prefix { 2 } else { 0 };
    let num_chars = width.max(nibbles.max(1) + prefix_chars);
    if cap < num_chars {
        return num_chars;
    }
    if num_chars == 0 {
        return 0;
    }

    core::ptr::write_bytes(out, b'0', num_chars);
    if prefix {
        *out.add(1) = b'x';
    }

    let table = if upper {
        &HEX_PAIRS_UPPER
    } else {
        &HEX_PAIRS_LOWER
    };
    let mut n = value;
    let mut pos = out.add(num_chars);
    while n >= 0x1_0000_0000 {
        pos = pos.sub(8);
        write_hex8(table, pos, (n & 0xFFFF_FFFF) as u32);
        n >>= 32;
    }
    if n >= 0x1000_0000 {
        pos = pos.sub(8);
        write_hex8(table, pos, n as u32);
    } else if n >= 0x1_0000 {
        let high = (n >> 16) as usize;
        let low = (n & 0xFFFF) as usize;
        let high_digits = if high >= 0x100 {
            3
        } else if high >= 0x10 {
            2
        } else {
            1
        };
        pos = pos.sub(high_digits + 4);
        if high_digits == 3 {
            *pos = *table.get_unchecked((high >> 8) * 2 + 1);
            write_hex2(table, pos.add(1), high & 0xFF);
        } else if high_digits == 2 {
            write_hex2(table, pos, high);
        } else {
            *pos = *table.get_unchecked(high * 2 + 1);
        }
        write_hex4(table, pos.add(high_digits), low);
    } else if n >= 0x1000 {
        pos = pos.sub(4);
        write_hex4(table, pos, n as usize);
    } else if n >= 0x100 {
        pos = pos.sub(3);
        *pos = *table.get_unchecked((n >> 8) as usize * 2 + 1);
        write_hex2(table, pos.add(1), (n & 0xFF) as usize);
    } else if n >= 0x10 {
        pos = pos.sub(2);
        write_hex2(table, pos, n as usize);
    } else if n != 0 {
        pos = pos.sub(1);
        *pos = *table.get_unchecked(n as usize * 2 + 1);
    }
    num_chars
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decimal(value: u64, grouping: bool) -> String {
        let mut buf = [0u8; 64];
        let len = unsafe {
            llvm_nativeformatting_format_decimal(value, grouping, buf.as_mut_ptr(), buf.len())
        };
        String::from_utf8(buf[..len].to_vec()).unwrap()
    }

    fn hex(value: u64, upper: bool, prefix: bool, width: usize) -> String {
        let mut buf = [0u8; 128];
        let len = unsafe {
            llvm_nativeformatting_format_hex(
                value,
                upper,
                prefix,
                width,
                buf.as_mut_ptr(),
                buf.len(),
            )
        };
        String::from_utf8(buf[..len].to_vec()).unwrap()
    }

    #[test]
    fn decimal_edges() {
        assert_eq!(decimal(0, false), "0");
        assert_eq!(decimal(1, false), "1");
        assert_eq!(decimal(9, false), "9");
        assert_eq!(decimal(10, false), "10");
        assert_eq!(decimal(99, false), "99");
        assert_eq!(decimal(100, false), "100");
        assert_eq!(decimal(u32::MAX as u64, false), "4294967295");
        assert_eq!(decimal(u64::MAX, false), "18446744073709551615");
    }

    #[test]
    fn decimal_grouping() {
        assert_eq!(decimal(1, true), "1");
        assert_eq!(decimal(999, true), "999");
        assert_eq!(decimal(1000, true), "1,000");
        assert_eq!(decimal(1234567, true), "1,234,567");
        assert_eq!(decimal(u64::MAX, true), "18,446,744,073,709,551,615");
    }

    #[test]
    fn hex_edges() {
        assert_eq!(hex(0, false, false, 0), "0");
        assert_eq!(hex(0, false, true, 0), "0x0");
        assert_eq!(hex(0, true, true, 0), "0x0");
        assert_eq!(hex(0xf, false, false, 0), "f");
        assert_eq!(hex(0xf, true, false, 0), "F");
        assert_eq!(hex(0x10, false, false, 0), "10");
        assert_eq!(hex(u64::MAX, false, false, 0), "ffffffffffffffff");
        assert_eq!(hex(u64::MAX, true, true, 0), "0xFFFFFFFFFFFFFFFF");
    }

    #[test]
    fn hex_width_and_prefix() {
        assert_eq!(hex(0, false, true, 4), "0x00");
        assert_eq!(hex(0xf, false, true, 6), "0x000f");
        assert_eq!(hex(0x1234, false, false, 8), "00001234");
        assert_eq!(hex(0x1234, true, true, 8), "0x001234");
    }

    #[test]
    fn short_buffer_returns_required_length() {
        let mut buf = [0u8; 2];
        let required = unsafe {
            llvm_nativeformatting_format_decimal(123456, false, buf.as_mut_ptr(), buf.len())
        };
        assert_eq!(required, 6);
        assert_eq!(buf, [0, 0]);

        let required = unsafe {
            llvm_nativeformatting_format_hex(0x1234, false, true, 0, buf.as_mut_ptr(), buf.len())
        };
        assert_eq!(required, 6);
        assert_eq!(buf, [0, 0]);
    }
}

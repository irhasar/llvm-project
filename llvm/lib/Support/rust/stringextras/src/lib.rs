//===-- lib.rs - Rust port of StringExtras case conversion ---------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
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

const UPPER: u8 = 1;
const LOWER: u8 = 2;
const DIGIT: u8 = 4;

const fn build_info() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let class = if i >= b'A' as usize && i <= b'Z' as usize {
            UPPER
        } else if i >= b'a' as usize && i <= b'z' as usize {
            LOWER
        } else if i >= b'0' as usize && i <= b'9' as usize {
            DIGIT
        } else {
            0
        };
        let lower = if i >= b'A' as usize && i <= b'Z' as usize {
            (i as u8) + (b'a' - b'A')
        } else {
            i as u8
        };
        table[i] = ((class as u16) << 8) | lower as u16;
        i += 1;
    }
    table
}

// Low byte: ASCII lower-case form. High byte: UPPER/LOWER/DIGIT class bits.
static INFO: [u16; 256] = build_info();

#[cfg(test)]
#[inline(always)]
fn class(byte: u8) -> u8 {
    (INFO[byte as usize] >> 8) as u8
}

#[cfg(test)]
#[inline(always)]
fn lower(byte: u8) -> u8 {
    INFO[byte as usize] as u8
}

#[inline(always)]
unsafe fn snake_into_unchecked(input: &[u8], output: *mut u8) -> usize {
    let len = input.len();
    let mut written = 0;
    let mut i = 0;
    if len == 0 {
        return 0;
    }

    let mut info0 = unsafe { *INFO.get_unchecked(*input.get_unchecked(0) as usize) };
    let mut info1 = if len > 1 {
        unsafe { *INFO.get_unchecked(*input.get_unchecked(1) as usize) }
    } else {
        0
    };
    let mut info2 = if len > 2 {
        unsafe { *INFO.get_unchecked(*input.get_unchecked(2) as usize) }
    } else {
        0
    };

    while i + 2 < len {
        unsafe { *output.add(written) = info0 as u8 };
        written += 1;
        let class = (info0 >> 8) as u8;
        let class1 = (info1 >> 8) as u8;
        let class2 = (info2 >> 8) as u8;
        let upper_run = (class & UPPER != 0) & (class1 & UPPER != 0) & (class2 & LOWER != 0);
        let lower_to_upper = (class & (LOWER | DIGIT) != 0) & (class1 & UPPER != 0);
        // Store unconditionally; the next lower-case byte overwrites this slot
        // when no underscore belongs here. Capacity is at least 2 * len.
        unsafe { *output.add(written) = b'_' };
        written += (upper_run | lower_to_upper) as usize;
        i += 1;
        info0 = info1;
        info1 = info2;
        if i + 2 < len {
            info2 = unsafe { *INFO.get_unchecked(*input.get_unchecked(i + 2) as usize) };
        }
    }

    while i < len {
        let info = unsafe { *INFO.get_unchecked(*input.get_unchecked(i) as usize) };
        unsafe { *output.add(written) = info as u8 };
        written += 1;
        let class = (info >> 8) as u8;
        if i + 1 < len {
            let info1 = unsafe { *INFO.get_unchecked(*input.get_unchecked(i + 1) as usize) };
            let class1 = (info1 >> 8) as u8;
            let lower_to_upper = (class & (LOWER | DIGIT) != 0) & (class1 & UPPER != 0);
            if lower_to_upper {
                unsafe { *output.add(written) = b'_' };
                written += 1;
            }
        }
        i += 1;
    }
    written
}

#[cfg(test)]
fn snake_into_checked(input: &[u8], output: &mut [u8]) -> usize {
    let mut written = 0;
    let mut i = 0;
    let len = input.len();
    while i < len {
        let byte = input[i];
        if written < output.len() {
            output[written] = lower(byte);
            written += 1;
        }

        let upper_run = i + 2 < len
            && class(byte) & UPPER != 0
            && class(input[i + 1]) & UPPER != 0
            && class(input[i + 2]) & LOWER != 0;
        let lower_to_upper =
            i + 1 < len && class(byte) & (LOWER | DIGIT) != 0 && class(input[i + 1]) & UPPER != 0;
        if (upper_run || lower_to_upper) && written < output.len() {
            output[written] = b'_';
            written += 1;
        }
        i += 1;
    }
    written
}

#[cfg(test)]
fn snake_into(input: &[u8], output: &mut [u8]) -> usize {
    if output.len() >= input.len().saturating_mul(2) {
        unsafe { snake_into_unchecked(input, output.as_mut_ptr()) }
    } else {
        snake_into_checked(input, output)
    }
}

#[inline(always)]
fn camel_lower(byte: u8) -> bool {
    byte.is_ascii_lowercase()
}

#[inline(always)]
fn camel_upper(byte: u8) -> u8 {
    if byte.is_ascii_lowercase() {
        byte - (b'a' - b'A')
    } else {
        byte
    }
}

#[inline(always)]
unsafe fn camel_into_unchecked(input: &[u8], capitalize_first: bool, output: *mut u8) -> usize {
    let len = input.len();
    if len == 0 {
        return 0;
    }

    let first = unsafe { *input.get_unchecked(0) };
    unsafe {
        *output = if capitalize_first && camel_lower(first) {
            camel_upper(first)
        } else {
            first
        };
    }
    let mut written = 1;
    let mut pos = 1;
    while pos < len {
        let byte = unsafe { *input.get_unchecked(pos) };
        if byte == b'_' && pos + 1 < len {
            let next = unsafe { *input.get_unchecked(pos + 1) };
            if camel_lower(next) {
                unsafe { *output.add(written) = camel_upper(next) };
                written += 1;
                pos += 2;
                continue;
            }
        }
        unsafe { *output.add(written) = byte };
        written += 1;
        pos += 1;
    }
    written
}

#[cfg(test)]
fn camel_into_checked(input: &[u8], capitalize_first: bool, output: &mut [u8]) -> usize {
    if input.is_empty() {
        return 0;
    }

    let mut written = 0;
    if written < output.len() {
        output[written] = if capitalize_first && camel_lower(input[0]) {
            camel_upper(input[0])
        } else {
            input[0]
        };
        written += 1;
    }

    let mut pos = 1;
    while pos < input.len() {
        if input[pos] == b'_' && pos + 1 < input.len() && camel_lower(input[pos + 1]) {
            pos += 1;
            if written < output.len() {
                output[written] = camel_upper(input[pos]);
                written += 1;
            }
        } else if written < output.len() {
            output[written] = input[pos];
            written += 1;
        }
        pos += 1;
    }
    written
}

#[cfg(test)]
fn camel_into(input: &[u8], capitalize_first: bool, output: &mut [u8]) -> usize {
    if output.len() >= input.len() {
        unsafe { camel_into_unchecked(input, capitalize_first, output.as_mut_ptr()) }
    } else {
        camel_into_checked(input, capitalize_first, output)
    }
}

/// Converts a camelCase or PascalCase ASCII string to snake_case.
///
/// # Safety
///
/// `src` must be readable for `src_len` bytes and `dst` must be writable for
/// `src_len * 2` bytes. Either pointer may be null when its length is zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_stringextras_snake(
    src: *const u8,
    src_len: usize,
    dst: *mut u8,
) -> usize {
    if src_len == 0 {
        return 0;
    }
    if src_len > usize::MAX / 2 {
        return 0;
    }
    let input = unsafe { core::slice::from_raw_parts(src, src_len) };
    unsafe { snake_into_unchecked(input, dst) }
}

/// Converts a snake_case ASCII string to camelCase or PascalCase.
///
/// # Safety
///
/// `src` must be readable for `src_len` bytes and `dst` must be writable for
/// `src_len` bytes. Either pointer may be null when its length is zero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_stringextras_camel(
    src: *const u8,
    src_len: usize,
    dst: *mut u8,
    capitalize_first: u8,
) -> usize {
    if src_len == 0 {
        return 0;
    }
    let input = unsafe { core::slice::from_raw_parts(src, src_len) };
    unsafe { camel_into_unchecked(input, capitalize_first != 0, dst) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snake(input: &[u8]) -> Vec<u8> {
        let mut output = vec![0; input.len() * 2];
        let written = snake_into(input, &mut output);
        output.truncate(written);
        output
    }

    fn camel(input: &[u8], capitalize_first: bool) -> Vec<u8> {
        let mut output = vec![0; input.len()];
        let written = camel_into(input, capitalize_first, &mut output);
        output.truncate(written);
        output
    }

    #[test]
    fn snake_handles_runs_and_boundaries() {
        assert_eq!(snake(b"HTTPServer"), b"http_server");
        assert_eq!(snake(b"ABCDef"), b"abc_def");
        assert_eq!(snake(b"foo123Bar"), b"foo123_bar");
        assert_eq!(snake(b"already_snake"), b"already_snake");
        assert_eq!(snake(b""), b"");
    }

    #[test]
    fn camel_handles_underscores() {
        assert_eq!(camel(b"http_server", false), b"httpServer");
        assert_eq!(camel(b"http_server", true), b"HttpServer");
        assert_eq!(camel(b"a__b", false), b"a_B");
        assert_eq!(camel(b"a_", false), b"a_");
        assert_eq!(camel(b"", true), b"");
    }

    #[test]
    fn high_bytes_are_unchanged() {
        assert_eq!(snake(&[0x80, 0xff]), &[0x80, 0xff]);
        assert_eq!(camel(&[0x80, 0xff], true), &[0x80, 0xff]);
    }

    #[test]
    fn capacity_is_respected() {
        let mut output = [0u8; 3];
        let written = snake_into(b"ABCDef", &mut output);
        assert_eq!(written, 3);
        assert_eq!(&output, b"abc");
    }
}

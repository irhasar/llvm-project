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

#[inline(always)]
fn is_upper(byte: u8) -> bool {
    byte.is_ascii_uppercase()
}

#[inline(always)]
fn is_lower(byte: u8) -> bool {
    byte.is_ascii_lowercase()
}

#[inline(always)]
fn is_digit(byte: u8) -> bool {
    byte.is_ascii_digit()
}

#[inline(always)]
fn lower(byte: u8) -> u8 {
    if is_upper(byte) {
        byte + (b'a' - b'A')
    } else {
        byte
    }
}

#[inline(always)]
fn upper(byte: u8) -> u8 {
    if is_lower(byte) {
        byte - (b'a' - b'A')
    } else {
        byte
    }
}

fn snake_into(input: &[u8], output: &mut [u8]) -> usize {
    let mut written = 0;
    let mut i = 0;
    while i < input.len() {
        let byte = input[i];
        if written < output.len() {
            output[written] = lower(byte);
            written += 1;
        }

        let upper_run = i + 2 < input.len()
            && is_upper(byte)
            && is_upper(input[i + 1])
            && is_lower(input[i + 2]);
        let lower_to_upper =
            i + 1 < input.len() && (is_lower(byte) || is_digit(byte)) && is_upper(input[i + 1]);
        if (upper_run || lower_to_upper) && written < output.len() {
            output[written] = b'_';
            written += 1;
        }
        i += 1;
    }
    written
}

fn camel_into(input: &[u8], capitalize_first: bool, output: &mut [u8]) -> usize {
    if input.is_empty() {
        return 0;
    }

    let mut written = 0;
    if written < output.len() {
        output[written] = if capitalize_first && is_lower(input[0]) {
            upper(input[0])
        } else {
            input[0]
        };
        written += 1;
    }

    let mut pos = 1;
    while pos < input.len() {
        if input[pos] == b'_' && pos + 1 < input.len() && is_lower(input[pos + 1]) {
            pos += 1;
            if written < output.len() {
                output[written] = upper(input[pos]);
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
    let output = unsafe { core::slice::from_raw_parts_mut(dst, src_len * 2) };
    snake_into(input, output)
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
    let output = unsafe { core::slice::from_raw_parts_mut(dst, src_len) };
    camel_into(input, capitalize_first != 0, output)
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

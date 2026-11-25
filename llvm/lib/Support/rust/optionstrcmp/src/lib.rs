//===-- lib.rs - Rust port of OptionStrCmp ---------------------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
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

#[inline(always)]
fn lower(byte: u8) -> u8 {
    if byte.is_ascii_uppercase() {
        byte + (b'a' - b'A')
    } else {
        byte
    }
}

#[inline(always)]
fn compare_bytes(a: &[u8], b: &[u8]) -> i32 {
    let min = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0;
    while i < min {
        let left = unsafe { *a.get_unchecked(i) };
        let right = unsafe { *b.get_unchecked(i) };
        if left != right {
            return if left < right { -1 } else { 1 };
        }
        i += 1;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_optionstrcmp_name(
    a: *const u8,
    a_len: usize,
    b: *const u8,
    b_len: usize,
    fallback_case_sensitive: u8,
) -> i32 {
    let a = if a_len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(a, a_len) }
    };
    let b = if b_len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(b, b_len) }
    };
    let min = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0;
    while i < min {
        let left = lower(unsafe { *a.get_unchecked(i) });
        let right = lower(unsafe { *b.get_unchecked(i) });
        if left != right {
            return if left < right { -1 } else { 1 };
        }
        i += 1;
    }

    if a.len() == b.len() {
        if fallback_case_sensitive != 0 {
            return compare_bytes(a, b);
        }
        return 0;
    }

    if a.len() == min { 1 } else { -1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmp(a: &[u8], b: &[u8], fallback: bool) -> i32 {
        unsafe { llvm_optionstrcmp_name(a.as_ptr(), a.len(), b.as_ptr(), b.len(), fallback as u8) }
    }

    #[test]
    fn case_insensitive_order() {
        assert_eq!(cmp(b"help", b"HELP", true), 1);
        assert_eq!(cmp(b"help", b"HELP", false), 0);
        assert_eq!(cmp(b"abc", b"ABD", true), -1);
        assert_eq!(cmp(b"ABD", b"abc", true), 1);
        assert_eq!(cmp(b"abc", b"abcd", true), 1);
        assert_eq!(cmp(b"abcd", b"abc", true), -1);
    }

    #[test]
    fn fallback_is_case_sensitive() {
        assert_eq!(cmp(b"HELP", b"help", true), -1);
        assert_eq!(cmp(b"help", b"HELP", true), 1);
        assert_eq!(cmp(b"HELP", b"help", false), 0);
    }

    #[test]
    fn high_bytes_are_unsigned() {
        assert_eq!(cmp(&[0xff], &[0x7f], true), 1);
        assert_eq!(cmp(&[0x80], &[0x81], true), -1);
    }

    #[test]
    fn empty_strings() {
        assert_eq!(cmp(b"", b"", true), 0);
        assert_eq!(cmp(b"", b"a", true), 1);
        assert_eq!(cmp(b"a", b"", true), -1);
    }
}

//===-- lib.rs - Rust port of VersionTuple parsing -------------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
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

pub const HAS_MINOR: u32 = 1;
pub const HAS_SUBMINOR: u32 = 2;
pub const HAS_BUILD: u32 = 4;
pub const HAS_SUBBUILD: u32 = 8;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ParsedVersion {
    pub major: u32,
    pub minor: u32,
    pub subminor: u32,
    pub build: u32,
    pub subbuild: u32,
    pub flags: u32,
}

#[inline(always)]
fn is_digit(byte: u8) -> bool {
    byte.wrapping_sub(b'0') < 10
}

#[inline(always)]
fn parse_component(input: &[u8], pos: &mut usize, value: &mut u32) -> bool {
    if *pos == input.len() {
        return true;
    }
    let first = input[*pos];
    *pos += 1;
    if !is_digit(first) {
        return true;
    }
    *value = (first - b'0') as u32;
    while *pos < input.len() {
        let byte = input[*pos];
        if !is_digit(byte) {
            break;
        }
        *value = value.wrapping_mul(10).wrapping_add((byte - b'0') as u32);
        *pos += 1;
    }
    false
}

#[inline(always)]
fn take_dot(input: &[u8], pos: &mut usize) -> bool {
    if *pos == input.len() || input[*pos] != b'.' {
        return true;
    }
    *pos += 1;
    false
}

/// Parses a version tuple into `out`.
///
/// # Safety
///
/// `data` must be readable for `len` bytes and may be null when `len` is zero.
/// `out` must be a valid, writable pointer to a `ParsedVersion`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_versiontuple_try_parse(
    data: *const u8,
    len: usize,
    out: *mut ParsedVersion,
) -> u8 {
    let input = if len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, len) }
    };
    let mut pos = 0usize;
    let mut major = 0u32;
    let mut minor = 0u32;
    let mut subminor = 0u32;
    let mut build = 0u32;
    let mut subbuild = 0u32;
    let mut flags = 0u32;

    if parse_component(input, &mut pos, &mut major) {
        return 1;
    }
    if pos == input.len() {
        unsafe {
            *out = ParsedVersion {
                major,
                minor: 0,
                subminor: 0,
                build: 0,
                subbuild: 0,
                flags: 0,
            }
        };
        return 0;
    }

    if take_dot(input, &mut pos) || parse_component(input, &mut pos, &mut minor) {
        return 1;
    }
    flags |= HAS_MINOR;
    if pos == input.len() {
        unsafe {
            *out = ParsedVersion {
                major,
                minor: minor & 0x7fff_ffff,
                subminor: 0,
                build: 0,
                subbuild: 0,
                flags,
            }
        };
        return 0;
    }

    if take_dot(input, &mut pos) || parse_component(input, &mut pos, &mut subminor) {
        return 1;
    }
    flags |= HAS_SUBMINOR;
    if pos == input.len() {
        unsafe {
            *out = ParsedVersion {
                major,
                minor: minor & 0x7fff_ffff,
                subminor: subminor & 0x7fff_ffff,
                build: 0,
                subbuild: 0,
                flags,
            }
        };
        return 0;
    }

    if take_dot(input, &mut pos) || parse_component(input, &mut pos, &mut build) {
        return 1;
    }
    if build >= 1024 * 1024 {
        return 1;
    }
    flags |= HAS_BUILD;
    if pos == input.len() {
        unsafe {
            *out = ParsedVersion {
                major,
                minor: minor & 0x7fff_ffff,
                subminor: subminor & 0x7fff_ffff,
                build,
                subbuild: 0,
                flags,
            }
        };
        return 0;
    }

    if take_dot(input, &mut pos) || parse_component(input, &mut pos, &mut subbuild) {
        return 1;
    }
    if subbuild >= 1024 || pos != input.len() {
        return 1;
    }
    flags |= HAS_SUBBUILD;
    unsafe {
        *out = ParsedVersion {
            major,
            minor: minor & 0x7fff_ffff,
            subminor: subminor & 0x7fff_ffff,
            build,
            subbuild,
            flags,
        }
    };
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &[u8]) -> Option<ParsedVersion> {
        let mut out = ParsedVersion::default();
        let status = unsafe { llvm_versiontuple_try_parse(input.as_ptr(), input.len(), &mut out) };
        if status == 0 { Some(out) } else { None }
    }

    #[test]
    fn basic_versions() {
        let v = parse(b"1.2.3.4.5").unwrap();
        assert_eq!(
            (v.major, v.minor, v.subminor, v.build, v.subbuild),
            (1, 2, 3, 4, 5)
        );
        assert_eq!(v.flags, HAS_MINOR | HAS_SUBMINOR | HAS_BUILD | HAS_SUBBUILD);
        let v = parse(b"17").unwrap();
        assert_eq!((v.major, v.flags), (17, 0));
    }

    #[test]
    fn rejects_bad_versions() {
        for input in [
            b"".as_slice(),
            b".1",
            b"1.",
            b"1..2",
            b"1.2.3.4.5.6",
            b"1.2.3.1048576",
            b"1.2.3.4.1024",
            b"1.2.3.4x",
            b"a",
        ] {
            assert!(parse(input).is_none(), "{:?}", input);
        }
    }

    #[test]
    fn wraps_large_components_like_cpp() {
        let v = parse(b"4294967296.2147483648.2147483649").unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 0);
        assert_eq!(v.subminor, 1);
    }
}

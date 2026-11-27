//===-- lib.rs - Rust port of LLVM's EBCDIC converter ----------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Bit-compatible reimplementation of llvm/lib/Support/ConvertEBCDIC.cpp.
// The C++ wrapper owns the SmallVector and passes its storage to these
// functions. Every function reports both the status and the number of bytes
// written, so a failed conversion leaves the same partial output as the C++
// implementation.
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

const OK: i32 = 0;
const ILLEGAL_BYTE_SEQUENCE: i32 = 1;
const INVALID_ARGUMENT: i32 = 2;
const OUTPUT_TOO_SMALL: i32 = 3;

const ISO88591_TO_IBM1047: [u8; 256] = [
    0x00, 0x01, 0x02, 0x03, 0x37, 0x2d, 0x2e, 0x2f, 0x16, 0x05, 0x15, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x3c, 0x3d, 0x32, 0x26, 0x18, 0x19, 0x3f, 0x27, 0x1c, 0x1d, 0x1e, 0x1f,
    0x40, 0x5a, 0x7f, 0x7b, 0x5b, 0x6c, 0x50, 0x7d, 0x4d, 0x5d, 0x5c, 0x4e, 0x6b, 0x60, 0x4b, 0x61,
    0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0x7a, 0x5e, 0x4c, 0x7e, 0x6e, 0x6f,
    0x7c, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6,
    0xd7, 0xd8, 0xd9, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xad, 0xe0, 0xbd, 0x5f, 0x6d,
    0x79, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96,
    0x97, 0x98, 0x99, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xc0, 0x4f, 0xd0, 0xa1, 0x07,
    0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x06, 0x17, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x09, 0x0a, 0x1b,
    0x30, 0x31, 0x1a, 0x33, 0x34, 0x35, 0x36, 0x08, 0x38, 0x39, 0x3a, 0x3b, 0x04, 0x14, 0x3e, 0xff,
    0x41, 0xaa, 0x4a, 0xb1, 0x9f, 0xb2, 0x6a, 0xb5, 0xbb, 0xb4, 0x9a, 0x8a, 0xb0, 0xca, 0xaf, 0xbc,
    0x90, 0x8f, 0xea, 0xfa, 0xbe, 0xa0, 0xb6, 0xb3, 0x9d, 0xda, 0x9b, 0x8b, 0xb7, 0xb8, 0xb9, 0xab,
    0x64, 0x65, 0x62, 0x66, 0x63, 0x67, 0x9e, 0x68, 0x74, 0x71, 0x72, 0x73, 0x78, 0x75, 0x76, 0x77,
    0xac, 0x69, 0xed, 0xee, 0xeb, 0xef, 0xec, 0xbf, 0x80, 0xfd, 0xfe, 0xfb, 0xfc, 0xba, 0xae, 0x59,
    0x44, 0x45, 0x42, 0x46, 0x43, 0x47, 0x9c, 0x48, 0x54, 0x51, 0x52, 0x53, 0x58, 0x55, 0x56, 0x57,
    0x8c, 0x49, 0xcd, 0xce, 0xcb, 0xcf, 0xcc, 0xe1, 0x70, 0xdd, 0xde, 0xdb, 0xdc, 0x8d, 0x8e, 0xdf,
];

const IBM1047_TO_ISO88591: [u8; 256] = [
    0x00, 0x01, 0x02, 0x03, 0x9c, 0x09, 0x86, 0x7f, 0x97, 0x8d, 0x8e, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x9d, 0x0a, 0x08, 0x87, 0x18, 0x19, 0x92, 0x8f, 0x1c, 0x1d, 0x1e, 0x1f,
    0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x17, 0x1b, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x05, 0x06, 0x07,
    0x90, 0x91, 0x16, 0x93, 0x94, 0x95, 0x96, 0x04, 0x98, 0x99, 0x9a, 0x9b, 0x14, 0x15, 0x9e, 0x1a,
    0x20, 0xa0, 0xe2, 0xe4, 0xe0, 0xe1, 0xe3, 0xe5, 0xe7, 0xf1, 0xa2, 0x2e, 0x3c, 0x28, 0x2b, 0x7c,
    0x26, 0xe9, 0xea, 0xeb, 0xe8, 0xed, 0xee, 0xef, 0xec, 0xdf, 0x21, 0x24, 0x2a, 0x29, 0x3b, 0x5e,
    0x2d, 0x2f, 0xc2, 0xc4, 0xc0, 0xc1, 0xc3, 0xc5, 0xc7, 0xd1, 0xa6, 0x2c, 0x25, 0x5f, 0x3e, 0x3f,
    0xf8, 0xc9, 0xca, 0xcb, 0xc8, 0xcd, 0xce, 0xcf, 0xcc, 0x60, 0x3a, 0x23, 0x40, 0x27, 0x3d, 0x22,
    0xd8, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0xab, 0xbb, 0xf0, 0xfd, 0xfe, 0xb1,
    0xb0, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f, 0x70, 0x71, 0x72, 0xaa, 0xba, 0xe6, 0xb8, 0xc6, 0xa4,
    0xb5, 0x7e, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0xa1, 0xbf, 0xd0, 0x5b, 0xde, 0xae,
    0xac, 0xa3, 0xa5, 0xb7, 0xa9, 0xa7, 0xb6, 0xbc, 0xbd, 0xbe, 0xdd, 0xa8, 0xaf, 0x5d, 0xb4, 0xd7,
    0x7b, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0xad, 0xf4, 0xf6, 0xf2, 0xf3, 0xf5,
    0x7d, 0x4a, 0x4b, 0x4c, 0x4d, 0x4e, 0x4f, 0x50, 0x51, 0x52, 0xb9, 0xfb, 0xfc, 0xf9, 0xfa, 0xff,
    0x5c, 0xf7, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5a, 0xb2, 0xd4, 0xd6, 0xd2, 0xd3, 0xd5,
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0xb3, 0xdb, 0xdc, 0xd9, 0xda, 0x9f,
];

/// Converts UTF-8 (only the two-byte subset accepted by the C++ code) to
/// EBCDIC-1047. Returns `(status, bytes_written)`.
#[inline(never)]
unsafe fn to_ebcdic(src: *const u8, len: usize, dst: *mut u8, cap: usize) -> (i32, usize) {
    let mut i = 0;
    let mut written = 0;
    if cap >= len {
        while i + 8 <= len {
            let word = u64::from_le(core::ptr::read_unaligned(src.add(i).cast::<u64>()));
            if word & 0x8080_8080_8080_8080 != 0 {
                break;
            }
            let mut out = 0u64;
            let mut j = 0;
            while j < 8 {
                let ch = ISO88591_TO_IBM1047[((word >> (j * 8)) & 0xff) as usize];
                out |= (ch as u64) << (j * 8);
                j += 1;
            }
            core::ptr::write_unaligned(dst.add(written).cast::<u64>(), out.to_le());
            i += 8;
            written += 8;
        }
        while i < len {
            let mut ch = *src.add(i);
            i += 1;
            if ch >= 128 {
                if ch != 0xc2 && ch != 0xc3 {
                    return (ILLEGAL_BYTE_SEQUENCE, written);
                }
                if i == len {
                    return (INVALID_ARGUMENT, written);
                }
                let ch2 = *src.add(i);
                i += 1;
                if ch2 & 0xc0 != 0x80 {
                    return (ILLEGAL_BYTE_SEQUENCE, written);
                }
                ch = ch2 | ch.wrapping_shl(6);
            }
            *dst.add(written) = ISO88591_TO_IBM1047[ch as usize];
            written += 1;
        }
        return (OK, written);
    }

    while i < len {
        let mut ch = *src.add(i);
        i += 1;
        if ch >= 128 {
            if ch != 0xc2 && ch != 0xc3 {
                return (ILLEGAL_BYTE_SEQUENCE, written);
            }
            if i == len {
                return (INVALID_ARGUMENT, written);
            }
            let ch2 = *src.add(i);
            i += 1;
            if ch2 & 0xc0 != 0x80 {
                return (ILLEGAL_BYTE_SEQUENCE, written);
            }
            ch = ch2 | ch.wrapping_shl(6);
        }
        if written == cap {
            return (OUTPUT_TOO_SMALL, written);
        }
        *dst.add(written) = ISO88591_TO_IBM1047[ch as usize];
        written += 1;
    }
    (OK, written)
}

/// Converts EBCDIC-1047 to UTF-8. Every byte is valid, so the only possible
/// failure is insufficient output space. Returns `(status, bytes_written,
/// bytes_consumed)`; on `OUTPUT_TOO_SMALL` the caller can resume at
/// `bytes_consumed` instead of restarting the conversion.
#[inline(never)]
unsafe fn to_utf8(src: *const u8, len: usize, dst: *mut u8, cap: usize) -> (i32, usize, usize) {
    let mut written = 0;
    let mut i = 0;
    if cap >= len.saturating_mul(2) {
        while i + 8 <= len {
            let word = u64::from_le(core::ptr::read_unaligned(src.add(i).cast::<u64>()));
            let mut out = 0u64;
            let mut j = 0;
            while j < 8 {
                let ch = IBM1047_TO_ISO88591[((word >> (j * 8)) & 0xff) as usize];
                if ch >= 128 {
                    break;
                }
                out |= (ch as u64) << (j * 8);
                j += 1;
            }
            if j != 8 {
                break;
            }
            core::ptr::write_unaligned(dst.add(written).cast::<u64>(), out.to_le());
            i += 8;
            written += 8;
        }
        while i < len {
            let ch = IBM1047_TO_ISO88591[*src.add(i) as usize];
            i += 1;
            if ch >= 128 {
                *dst.add(written) = 0xc0 | (ch >> 6);
                *dst.add(written + 1) = ch & 0xbf;
                written += 2;
            } else {
                *dst.add(written) = ch;
                written += 1;
            }
        }
        return (OK, written, len);
    }

    if cap >= len {
        while i < len {
            let ch = IBM1047_TO_ISO88591[*src.add(i) as usize];
            i += 1;
            if ch >= 128 {
                if written + 2 > cap {
                    return (OUTPUT_TOO_SMALL, written, i - 1);
                }
                *dst.add(written) = 0xc0 | (ch >> 6);
                *dst.add(written + 1) = ch & 0xbf;
                written += 2;
            } else {
                if written == cap {
                    return (OUTPUT_TOO_SMALL, written, i - 1);
                }
                *dst.add(written) = ch;
                written += 1;
            }
        }
        return (OK, written, len);
    }

    while i < len {
        let ch = IBM1047_TO_ISO88591[*src.add(i) as usize];
        i += 1;
        let needed = if ch >= 128 { 2 } else { 1 };
        if cap - written < needed {
            return (OUTPUT_TOO_SMALL, written, i - 1);
        }
        if ch >= 128 {
            *dst.add(written) = 0xc0 | (ch >> 6);
            *dst.add(written + 1) = ch & 0xbf;
        } else {
            *dst.add(written) = ch;
        }
        written += needed;
    }
    (OK, written, len)
}

/// Converts UTF-8 input for the U+0000..U+00FF repertoire (one- and two-byte
/// sequences only) to EBCDIC-1047.
///
/// # Safety
///
/// `src` must be readable for `len` bytes and may be null when `len` is zero.
/// `dst` must be writable for `cap` bytes and may be null when `cap` is zero.
/// `out_len` must be a valid, writable pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_convertebcdic_to_ebcdic(
    src: *const u8,
    len: usize,
    dst: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> i32 {
    let (status, written) = to_ebcdic(src, len, dst, cap);
    *out_len = written;
    status
}

/// Converts EBCDIC-1047 input to UTF-8.
///
/// `*out_consumed` receives the number of input bytes converted. When the
/// status is `OUTPUT_TOO_SMALL`, the caller can retry from that offset with the
/// remaining output buffer instead of redoing the prefix.
///
/// # Safety
///
/// `src` must be readable for `len` bytes and may be null when `len` is zero.
/// `dst` must be writable for `cap` bytes and may be null when `cap` is zero.
/// `out_len` and `out_consumed` must be valid, writable pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_convertebcdic_to_utf8(
    src: *const u8,
    len: usize,
    dst: *mut u8,
    cap: usize,
    out_len: *mut usize,
    out_consumed: *mut usize,
) -> i32 {
    let (status, written, consumed) = to_utf8(src, len, dst, cap);
    *out_len = written;
    *out_consumed = consumed;
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_ebcdic_vec(input: &[u8]) -> (i32, Vec<u8>) {
        let mut out = vec![0u8; input.len()];
        let mut len = 0usize;
        let status = unsafe {
            llvm_convertebcdic_to_ebcdic(
                input.as_ptr(),
                input.len(),
                out.as_mut_ptr(),
                out.len(),
                &mut len,
            )
        };
        out.truncate(len);
        (status, out)
    }

    fn to_utf8_vec(input: &[u8]) -> (i32, Vec<u8>) {
        let mut out = vec![0u8; input.len() * 2];
        let mut len = 0usize;
        let mut consumed = 0usize;
        let status = unsafe {
            llvm_convertebcdic_to_utf8(
                input.as_ptr(),
                input.len(),
                out.as_mut_ptr(),
                out.len(),
                &mut len,
                &mut consumed,
            )
        };
        assert_eq!(consumed, input.len());
        out.truncate(len);
        (status, out)
    }

    #[test]
    fn ascii_golden() {
        assert_eq!(to_ebcdic_vec(b"ABC"), (OK, vec![0xc1, 0xc2, 0xc3]));
        assert_eq!(to_utf8_vec(&[0xc1, 0xc2, 0xc3]), (OK, b"ABC".to_vec()));
    }

    #[test]
    fn two_byte_utf8() {
        assert_eq!(to_ebcdic_vec("é".as_bytes()), (OK, vec![0x51]));
        assert_eq!(to_utf8_vec(&[0x51]), (OK, "é".as_bytes().to_vec()));
    }

    #[test]
    fn errors_keep_partial_output() {
        assert_eq!(to_ebcdic_vec(&[0x80]), (ILLEGAL_BYTE_SEQUENCE, vec![]));
        assert_eq!(to_ebcdic_vec(&[0xc2]), (INVALID_ARGUMENT, vec![]));
        assert_eq!(
            to_ebcdic_vec(&[0xc2, 0x41]),
            (ILLEGAL_BYTE_SEQUENCE, vec![])
        );
        assert_eq!(
            to_ebcdic_vec(&[0x41, 0xc2, 0x41]),
            (ILLEGAL_BYTE_SEQUENCE, vec![0xc1])
        );
    }

    #[test]
    fn all_ebcdic_bytes_round_trip() {
        let input: Vec<u8> = (0..=255).collect();
        let (status, utf8) = to_utf8_vec(&input);
        assert_eq!(status, OK);
        let (status, ebcdic) = to_ebcdic_vec(&utf8);
        assert_eq!(status, OK);
        assert_eq!(ebcdic, input);
    }

    #[test]
    fn output_too_small() {
        let input = [0x51u8];
        let mut out = [0u8; 1];
        let mut len = 0;
        let mut consumed = 0;
        let status = unsafe {
            llvm_convertebcdic_to_utf8(
                input.as_ptr(),
                1,
                out.as_mut_ptr(),
                1,
                &mut len,
                &mut consumed,
            )
        };
        assert_eq!((status, len, consumed), (OUTPUT_TOO_SMALL, 0, 0));
    }
}

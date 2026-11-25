//===-- lib.rs - Rust port of LLVM's ConvertUTF -----------------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Bit-compatible reimplementation of the conversions in
// llvm/lib/Support/ConvertUTF.cpp (Unicode 6.3, D93b).
//
//===----------------------------------------------------------------------===//

#![cfg_attr(not(test), no_std)]
// The extern "C" entry points document their pointer contracts in the C
// headers; repeating them as Rust `# Safety` sections would be redundant.
#![allow(clippy::missing_safety_doc)]
// The range checks mirror the C++ source one-for-one; keeping the explicit
// comparisons makes the translation easier to audit.
#![allow(clippy::manual_range_contains)]

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    unsafe extern "C" {
        fn abort() -> !;
    }
    unsafe { abort() }
}

const UNI_REPLACEMENT_CHAR: u32 = 0x0000_FFFD;
const UNI_MAX_BMP: u32 = 0x0000_FFFF;
const UNI_MAX_UTF16: u32 = 0x0010_FFFF;
const UNI_MAX_LEGAL_UTF32: u32 = 0x0010_FFFF;
const UNI_SUR_HIGH_START: u32 = 0xD800;
const UNI_SUR_HIGH_END: u32 = 0xDBFF;
const UNI_SUR_LOW_START: u32 = 0xDC00;
const UNI_SUR_LOW_END: u32 = 0xDFFF;
const HALF_SHIFT: u32 = 10;
const HALF_BASE: u32 = 0x0001_0000;
const HALF_MASK: u32 = 0x3FF;

const CONVERSION_OK: u32 = 0;
const SOURCE_EXHAUSTED: u32 = 1;
const TARGET_EXHAUSTED: u32 = 2;
const SOURCE_ILLEGAL: u32 = 3;
const STRICT_CONVERSION: u32 = 0;

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

#[inline(always)]
fn is_continuation(b: u8) -> bool {
    b >= 0x80 && b <= 0xBF
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
    if b1 >= 0xC2 && b1 <= 0xDF {
        return 1;
    }
    if p == source_end {
        return 1;
    }

    let b2 = unsafe { *p };
    p = unsafe { p.add(1) };

    if b1 == 0xE0 {
        return if b2 >= 0xA0 && b2 <= 0xBF { 2 } else { 1 };
    }
    if b1 >= 0xE1 && b1 <= 0xEC {
        return if b2 >= 0x80 && b2 <= 0xBF { 2 } else { 1 };
    }
    if b1 == 0xED {
        return if b2 >= 0x80 && b2 <= 0x9F { 2 } else { 1 };
    }
    if b1 >= 0xEE && b1 <= 0xEF {
        return if b2 >= 0x80 && b2 <= 0xBF { 2 } else { 1 };
    }
    if b1 == 0xF0 {
        if b2 >= 0x90 && b2 <= 0xBF {
            if p == source_end {
                return 2;
            }
            let b3 = unsafe { *p };
            return if b3 >= 0x80 && b3 <= 0xBF { 3 } else { 2 };
        }
        return 1;
    }
    if b1 >= 0xF1 && b1 <= 0xF3 {
        if b2 >= 0x80 && b2 <= 0xBF {
            if p == source_end {
                return 2;
            }
            let b3 = unsafe { *p };
            return if b3 >= 0x80 && b3 <= 0xBF { 3 } else { 2 };
        }
        return 1;
    }
    if b1 == 0xF4 {
        if b2 >= 0x80 && b2 <= 0x8F {
            if p == source_end {
                return 2;
            }
            let b3 = unsafe { *p };
            return if b3 >= 0x80 && b3 <= 0xBF { 3 } else { 2 };
        }
        return 1;
    }

    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_convertutf_get_num_bytes_for_utf8(first: u8) -> u32 {
    TRAILING_BYTES[first as usize] as u32 + 1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_convertutf_is_legal_utf8_sequence(
    source: *const u8,
    source_end: *const u8,
) -> u8 {
    let length = TRAILING_BYTES[unsafe { *source } as usize] as usize + 1;
    let avail = unsafe { source_end.offset_from(source) };
    if length as isize > avail {
        return 0;
    }
    unsafe { is_legal_utf8(source, length) as u8 }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_convertutf_get_utf8_sequence_size(
    source: *const u8,
    source_end: *const u8,
) -> u32 {
    let length = TRAILING_BYTES[unsafe { *source } as usize] as usize + 1;
    let avail = unsafe { source_end.offset_from(source) };
    if length as isize <= avail && unsafe { is_legal_utf8(source, length) } {
        length as u32
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_convertutf_find_maximal_subpart(
    source: *const u8,
    source_end: *const u8,
) -> u32 {
    unsafe { find_maximal_subpart(source, source_end) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_convertutf_is_legal_utf8_string(
    source: *mut *const u8,
    source_end: *const u8,
) -> u8 {
    let mut s = unsafe { *source };
    loop {
        while s != source_end && unsafe { *s } < 0x80 {
            s = unsafe { s.add(1) };
        }
        if s == source_end {
            unsafe { *source = s };
            return 1;
        }

        let length = TRAILING_BYTES[unsafe { *s } as usize] as usize + 1;
        if length as isize > unsafe { source_end.offset_from(s) }
            || !unsafe { is_legal_utf8(s, length) }
        {
            unsafe { *source = s };
            return 0;
        }
        s = unsafe { s.add(length) };
    }
}

#[inline(always)]
fn is_utf16_surrogate(ch: u32) -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        let tmp: u32;
        unsafe {
            core::arch::asm!(
                "lea {tmp:e}, [{ch:e} - 0xd800]",
                tmp = out(reg) tmp,
                ch = in(reg) ch,
                options(pure, nomem, nostack),
            );
        }
        tmp <= 0x7FF
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        ch.wrapping_sub(UNI_SUR_HIGH_START) <= UNI_SUR_LOW_END - UNI_SUR_HIGH_START
    }
}

#[inline(never)]
unsafe fn convert_ascii_run_utf16(
    source: &mut *const u32,
    source_end: *const u32,
    target: &mut *mut u16,
    target_end: *mut u16,
) {
    unsafe {
        let mut s = *source;
        let mut t = *target;
        #[cfg(target_endian = "little")]
        {
            while !s.is_null()
                && !t.is_null()
                && source_end.offset_from(s) >= 4
                && target_end.offset_from(t) >= 4
            {
                let c0 = *s;
                let c1 = *s.add(1);
                let c2 = *s.add(2);
                let c3 = *s.add(3);
                if (c0 | c1 | c2 | c3) >= 0x80 {
                    break;
                }
                let packed =
                    (c0 as u64) | ((c1 as u64) << 16) | ((c2 as u64) << 32) | ((c3 as u64) << 48);
                (t as *mut u64).write_unaligned(packed);
                s = s.add(4);
                t = t.add(4);
            }
        }
        while s < source_end && t < target_end && *s < 0x80 {
            *t = *s as u16;
            s = s.add(1);
            t = t.add(1);
        }
        *source = s;
        *target = t;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_convertutf_utf32_to_utf16(
    source_start: *mut *const u32,
    source_end: *const u32,
    target_start: *mut *mut u16,
    target_end: *mut u16,
    flags: u32,
) -> u32 {
    let mut source = unsafe { *source_start };
    let mut target = unsafe { *target_start };
    let mut result = CONVERSION_OK;

    let mut ascii_run = false;
    if source < source_end && unsafe { source.add(1) } < source_end {
        let c0 = unsafe { *source };
        let c1 = unsafe { *source.add(1) };
        ascii_run = (c0 | c1) < 0x80;
    }
    if ascii_run {
        unsafe { convert_ascii_run_utf16(&mut source, source_end, &mut target, target_end) };
    } else if source < source_end && target < target_end && unsafe { *source } < 0x80 {
        unsafe { *target = *source as u16 };
        source = unsafe { source.add(1) };
        target = unsafe { target.add(1) };
    }
    // With two output slots per input code point the worst case cannot
    // exhaust the target, so the hot loop can skip the capacity checks.
    let src_len = if source == source_end {
        0
    } else {
        (unsafe { source_end.offset_from(source) }) as usize
    };
    let dst_len = if target == target_end {
        0
    } else {
        (unsafe { target_end.offset_from(target) }) as usize
    };
    if dst_len / 2 >= src_len {
        while source < source_end {
            let ch = unsafe { *source };
            source = unsafe { source.add(1) };
            if ch <= UNI_MAX_BMP {
                if is_utf16_surrogate(ch) {
                    if flags == STRICT_CONVERSION {
                        source = unsafe { source.sub(1) };
                        result = SOURCE_ILLEGAL;
                        break;
                    } else {
                        unsafe { *target = UNI_REPLACEMENT_CHAR as u16 };
                        target = unsafe { target.add(1) };
                    }
                } else {
                    unsafe { *target = ch as u16 };
                    target = unsafe { target.add(1) };
                }
            } else if ch > UNI_MAX_LEGAL_UTF32 {
                if flags == STRICT_CONVERSION {
                    result = SOURCE_ILLEGAL;
                } else {
                    unsafe { *target = UNI_REPLACEMENT_CHAR as u16 };
                    target = unsafe { target.add(1) };
                }
            } else {
                let ch = ch - HALF_BASE;
                let hi = ((ch >> HALF_SHIFT) + UNI_SUR_HIGH_START) as u16;
                let lo = ((ch & HALF_MASK) + UNI_SUR_LOW_START) as u16;
                #[cfg(target_endian = "little")]
                unsafe {
                    (target as *mut u32).write_unaligned((hi as u32) | ((lo as u32) << 16));
                }
                #[cfg(not(target_endian = "little"))]
                unsafe {
                    *target = hi;
                    *target.add(1) = lo;
                }
                target = unsafe { target.add(2) };
            }
        }
        unsafe { *source_start = source };
        unsafe { *target_start = target };
        return result;
    }
    while source < source_end {
        if target >= target_end {
            result = TARGET_EXHAUSTED;
            break;
        }

        let ch = unsafe { *source };
        source = unsafe { source.add(1) };
        if ch <= UNI_MAX_BMP {
            if is_utf16_surrogate(ch) {
                if flags == STRICT_CONVERSION {
                    source = unsafe { source.sub(1) };
                    result = SOURCE_ILLEGAL;
                    break;
                } else {
                    unsafe { *target = UNI_REPLACEMENT_CHAR as u16 };
                    target = unsafe { target.add(1) };
                }
            } else {
                unsafe { *target = ch as u16 };
                target = unsafe { target.add(1) };
            }
        } else if ch > UNI_MAX_LEGAL_UTF32 {
            if flags == STRICT_CONVERSION {
                result = SOURCE_ILLEGAL;
            } else {
                unsafe { *target = UNI_REPLACEMENT_CHAR as u16 };
                target = unsafe { target.add(1) };
            }
        } else {
            if unsafe { target.add(1) } >= target_end {
                source = unsafe { source.sub(1) };
                result = TARGET_EXHAUSTED;
                break;
            }
            let ch = ch - HALF_BASE;
            unsafe { *target = ((ch >> HALF_SHIFT) + UNI_SUR_HIGH_START) as u16 };
            target = unsafe { target.add(1) };
            unsafe { *target = ((ch & HALF_MASK) + UNI_SUR_LOW_START) as u16 };
            target = unsafe { target.add(1) };
        }
    }

    unsafe { *source_start = source };
    unsafe { *target_start = target };
    result
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_convertutf_utf16_to_utf32(
    source_start: *mut *const u16,
    source_end: *const u16,
    target_start: *mut *mut u32,
    target_end: *mut u32,
    flags: u32,
) -> u32 {
    let mut source = unsafe { *source_start };
    let mut target = unsafe { *target_start };
    let mut result = CONVERSION_OK;

    while source < source_end && target < target_end && unsafe { *source } < 0x80 {
        unsafe { *target = *source as u32 };
        source = unsafe { source.add(1) };
        target = unsafe { target.add(1) };
    }
    while source < source_end {
        let old_source = source;
        let mut ch = unsafe { *source } as u32;
        source = unsafe { source.add(1) };

        if ch >= UNI_SUR_HIGH_START && ch <= UNI_SUR_HIGH_END {
            if source < source_end {
                let ch2 = unsafe { *source } as u32;
                if ch2 >= UNI_SUR_LOW_START && ch2 <= UNI_SUR_LOW_END {
                    ch = ((ch - UNI_SUR_HIGH_START) << HALF_SHIFT)
                        + (ch2 - UNI_SUR_LOW_START)
                        + HALF_BASE;
                    source = unsafe { source.add(1) };
                } else if flags == STRICT_CONVERSION {
                    source = unsafe { source.sub(1) };
                    result = SOURCE_ILLEGAL;
                    break;
                }
            } else {
                source = unsafe { source.sub(1) };
                result = SOURCE_EXHAUSTED;
                break;
            }
        } else if flags == STRICT_CONVERSION && ch >= UNI_SUR_LOW_START && ch <= UNI_SUR_LOW_END {
            source = unsafe { source.sub(1) };
            result = SOURCE_ILLEGAL;
            break;
        }

        if target >= target_end {
            source = old_source;
            result = TARGET_EXHAUSTED;
            break;
        }
        unsafe { *target = ch };
        target = unsafe { target.add(1) };
    }

    unsafe { *source_start = source };
    unsafe { *target_start = target };
    result
}

#[inline(always)]
unsafe fn write_utf8(ch: u32, target: *mut u8, bytes: usize) {
    match bytes {
        1 => unsafe { *target = ch as u8 },
        2 => {
            unsafe { *target = (0xC0 | (ch >> 6)) as u8 };
            unsafe { *target.add(1) = (0x80 | (ch & 0x3F)) as u8 };
        }
        3 => {
            unsafe { *target = (0xE0 | (ch >> 12)) as u8 };
            unsafe { *target.add(1) = (0x80 | ((ch >> 6) & 0x3F)) as u8 };
            unsafe { *target.add(2) = (0x80 | (ch & 0x3F)) as u8 };
        }
        _ => {
            unsafe { *target = (0xF0 | (ch >> 18)) as u8 };
            unsafe { *target.add(1) = (0x80 | ((ch >> 12) & 0x3F)) as u8 };
            unsafe { *target.add(2) = (0x80 | ((ch >> 6) & 0x3F)) as u8 };
            unsafe { *target.add(3) = (0x80 | (ch & 0x3F)) as u8 };
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_convertutf_utf16_to_utf8(
    source_start: *mut *const u16,
    source_end: *const u16,
    target_start: *mut *mut u8,
    target_end: *mut u8,
    flags: u32,
) -> u32 {
    let mut source = unsafe { *source_start };
    let mut target = unsafe { *target_start };
    let mut result = CONVERSION_OK;

    while source < source_end && target < target_end && unsafe { *source } < 0x80 {
        unsafe { *target = *source as u8 };
        source = unsafe { source.add(1) };
        target = unsafe { target.add(1) };
    }
    while source < source_end {
        let old_source = source;
        let mut ch = unsafe { *source } as u32;
        source = unsafe { source.add(1) };

        if ch >= UNI_SUR_HIGH_START && ch <= UNI_SUR_HIGH_END {
            if source < source_end {
                let ch2 = unsafe { *source } as u32;
                if ch2 >= UNI_SUR_LOW_START && ch2 <= UNI_SUR_LOW_END {
                    ch = ((ch - UNI_SUR_HIGH_START) << HALF_SHIFT)
                        + (ch2 - UNI_SUR_LOW_START)
                        + HALF_BASE;
                    source = unsafe { source.add(1) };
                } else if flags == STRICT_CONVERSION {
                    source = unsafe { source.sub(1) };
                    result = SOURCE_ILLEGAL;
                    break;
                }
            } else {
                source = unsafe { source.sub(1) };
                result = SOURCE_EXHAUSTED;
                break;
            }
        } else if flags == STRICT_CONVERSION && ch >= UNI_SUR_LOW_START && ch <= UNI_SUR_LOW_END {
            source = unsafe { source.sub(1) };
            result = SOURCE_ILLEGAL;
            break;
        }

        let bytes = if ch < 0x80 {
            1
        } else if ch < 0x800 {
            2
        } else if ch < 0x10000 {
            3
        } else if ch < 0x110000 {
            4
        } else {
            ch = UNI_REPLACEMENT_CHAR;
            3
        };

        let new_target = unsafe { target.add(bytes) };
        if new_target > target_end {
            source = old_source;
            result = TARGET_EXHAUSTED;
            break;
        }
        unsafe { write_utf8(ch, target, bytes) };
        target = new_target;
    }

    unsafe { *source_start = source };
    unsafe { *target_start = target };
    result
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_convertutf_utf32_to_utf8(
    source_start: *mut *const u32,
    source_end: *const u32,
    target_start: *mut *mut u8,
    target_end: *mut u8,
    flags: u32,
) -> u32 {
    let mut source = unsafe { *source_start };
    let mut target = unsafe { *target_start };
    let mut result = CONVERSION_OK;

    while source < source_end && target < target_end && unsafe { *source } < 0x80 {
        unsafe { *target = *source as u8 };
        source = unsafe { source.add(1) };
        target = unsafe { target.add(1) };
    }
    while source < source_end {
        let mut ch = unsafe { *source };
        source = unsafe { source.add(1) };

        if flags == STRICT_CONVERSION && ch >= UNI_SUR_HIGH_START && ch <= UNI_SUR_LOW_END {
            source = unsafe { source.sub(1) };
            result = SOURCE_ILLEGAL;
            break;
        }

        let bytes = if ch < 0x80 {
            1
        } else if ch < 0x800 {
            2
        } else if ch < 0x10000 {
            3
        } else if ch <= UNI_MAX_LEGAL_UTF32 {
            4
        } else {
            ch = UNI_REPLACEMENT_CHAR;
            result = SOURCE_ILLEGAL;
            3
        };

        let new_target = unsafe { target.add(bytes) };
        if new_target > target_end {
            source = unsafe { source.sub(1) };
            result = TARGET_EXHAUSTED;
            break;
        }
        unsafe { write_utf8(ch, target, bytes) };
        target = new_target;
    }

    unsafe { *source_start = source };
    unsafe { *target_start = target };
    result
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_convertutf_utf8_to_utf16(
    source_start: *mut *const u8,
    source_end: *const u8,
    target_start: *mut *mut u16,
    target_end: *mut u16,
    flags: u32,
) -> u32 {
    let mut source = unsafe { *source_start };
    let mut target = unsafe { *target_start };
    let mut result = CONVERSION_OK;

    while source < source_end {
        if unsafe { *source } < 0x80 {
            if target >= target_end {
                result = TARGET_EXHAUSTED;
                break;
            }
            unsafe { *target = *source as u16 };
            source = unsafe { source.add(1) };
            target = unsafe { target.add(1) };
            continue;
        }

        let extra = TRAILING_BYTES[unsafe { *source } as usize] as usize;
        if extra as isize >= unsafe { source_end.offset_from(source) } {
            result = SOURCE_EXHAUSTED;
            break;
        }
        if !unsafe { is_legal_utf8(source, extra + 1) } {
            result = SOURCE_ILLEGAL;
            break;
        }

        let ch = unsafe { decode_utf8(source, extra) };
        source = unsafe { source.add(extra + 1) };

        if target >= target_end {
            source = unsafe { source.sub(extra + 1) };
            result = TARGET_EXHAUSTED;
            break;
        }
        if ch <= UNI_MAX_BMP {
            if ch >= UNI_SUR_HIGH_START && ch <= UNI_SUR_LOW_END {
                if flags == STRICT_CONVERSION {
                    source = unsafe { source.sub(extra + 1) };
                    result = SOURCE_ILLEGAL;
                    break;
                } else {
                    unsafe { *target = UNI_REPLACEMENT_CHAR as u16 };
                    target = unsafe { target.add(1) };
                }
            } else {
                unsafe { *target = ch as u16 };
                target = unsafe { target.add(1) };
            }
        } else if ch > UNI_MAX_UTF16 {
            if flags == STRICT_CONVERSION {
                source = unsafe { source.sub(extra + 1) };
                result = SOURCE_ILLEGAL;
                break;
            } else {
                unsafe { *target = UNI_REPLACEMENT_CHAR as u16 };
                target = unsafe { target.add(1) };
            }
        } else {
            if unsafe { target.add(1) } >= target_end {
                source = unsafe { source.sub(extra + 1) };
                result = TARGET_EXHAUSTED;
                break;
            }
            let ch = ch - HALF_BASE;
            unsafe { *target = ((ch >> HALF_SHIFT) + UNI_SUR_HIGH_START) as u16 };
            target = unsafe { target.add(1) };
            unsafe { *target = ((ch & HALF_MASK) + UNI_SUR_LOW_START) as u16 };
            target = unsafe { target.add(1) };
        }
    }

    unsafe { *source_start = source };
    unsafe { *target_start = target };
    result
}

unsafe fn convert_utf8_to_utf32_impl(
    source_start: *mut *const u8,
    source_end: *const u8,
    target_start: *mut *mut u32,
    target_end: *mut u32,
    flags: u32,
    input_is_partial: bool,
) -> u32 {
    let mut source = unsafe { *source_start };
    let mut target = unsafe { *target_start };
    let mut result = CONVERSION_OK;

    while source < source_end {
        if unsafe { *source } < 0x80 {
            if target >= target_end {
                result = TARGET_EXHAUSTED;
                break;
            }
            unsafe { *target = *source as u32 };
            source = unsafe { source.add(1) };
            target = unsafe { target.add(1) };
            continue;
        }

        let extra = TRAILING_BYTES[unsafe { *source } as usize] as usize;
        if extra as isize >= unsafe { source_end.offset_from(source) } {
            if flags == STRICT_CONVERSION || input_is_partial {
                result = SOURCE_EXHAUSTED;
                break;
            }
            if target >= target_end {
                result = TARGET_EXHAUSTED;
                break;
            }
            result = SOURCE_ILLEGAL;
            let n = unsafe { find_maximal_subpart(source, source_end) } as usize;
            source = unsafe { source.add(n) };
            unsafe { *target = UNI_REPLACEMENT_CHAR };
            target = unsafe { target.add(1) };
            continue;
        }

        if target >= target_end {
            result = TARGET_EXHAUSTED;
            break;
        }

        if !unsafe { is_legal_utf8(source, extra + 1) } {
            result = SOURCE_ILLEGAL;
            if flags == STRICT_CONVERSION {
                break;
            }
            let n = unsafe { find_maximal_subpart(source, source_end) } as usize;
            source = unsafe { source.add(n) };
            unsafe { *target = UNI_REPLACEMENT_CHAR };
            target = unsafe { target.add(1) };
            continue;
        }

        let ch = unsafe { decode_utf8(source, extra) };
        source = unsafe { source.add(extra + 1) };

        if ch <= UNI_MAX_LEGAL_UTF32 {
            if ch >= UNI_SUR_HIGH_START && ch <= UNI_SUR_LOW_END {
                if flags == STRICT_CONVERSION {
                    source = unsafe { source.sub(extra + 1) };
                    result = SOURCE_ILLEGAL;
                    break;
                }
                unsafe { *target = UNI_REPLACEMENT_CHAR };
                target = unsafe { target.add(1) };
            } else {
                unsafe { *target = ch };
                target = unsafe { target.add(1) };
            }
        } else {
            result = SOURCE_ILLEGAL;
            unsafe { *target = UNI_REPLACEMENT_CHAR };
            target = unsafe { target.add(1) };
        }
    }

    unsafe { *source_start = source };
    unsafe { *target_start = target };
    result
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_convertutf_utf8_to_utf32_partial(
    source_start: *mut *const u8,
    source_end: *const u8,
    target_start: *mut *mut u32,
    target_end: *mut u32,
    flags: u32,
) -> u32 {
    unsafe {
        convert_utf8_to_utf32_impl(
            source_start,
            source_end,
            target_start,
            target_end,
            flags,
            true,
        )
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_convertutf_utf8_to_utf32(
    source_start: *mut *const u8,
    source_end: *const u8,
    target_start: *mut *mut u32,
    target_end: *mut u32,
    flags: u32,
) -> u32 {
    unsafe {
        convert_utf8_to_utf32_impl(
            source_start,
            source_end,
            target_start,
            target_end,
            flags,
            false,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn cvt8to16(input: &[u8], flags: u32, cap: usize) -> (u32, usize, usize, Vec<u16>) {
        let mut out = std::vec![0u16; cap];
        let mut src = input.as_ptr();
        let mut dst = out.as_mut_ptr();
        let r = unsafe {
            llvm_convertutf_utf8_to_utf16(
                &mut src,
                input.as_ptr().add(input.len()),
                &mut dst,
                out.as_mut_ptr().add(cap),
                flags,
            )
        };
        (
            r,
            src as usize - input.as_ptr() as usize,
            (dst as usize - out.as_ptr() as usize) / 2,
            out,
        )
    }

    fn cvt8to32(input: &[u8], flags: u32, cap: usize) -> (u32, usize, usize, Vec<u32>) {
        let mut out = std::vec![0u32; cap];
        let mut src = input.as_ptr();
        let mut dst = out.as_mut_ptr();
        let r = unsafe {
            llvm_convertutf_utf8_to_utf32(
                &mut src,
                input.as_ptr().add(input.len()),
                &mut dst,
                out.as_mut_ptr().add(cap),
                flags,
            )
        };
        (
            r,
            src as usize - input.as_ptr() as usize,
            (dst as usize - out.as_ptr() as usize) / 4,
            out,
        )
    }

    fn cvt16to8(input: &[u16], flags: u32, cap: usize) -> (u32, usize, usize, Vec<u8>) {
        let mut out = std::vec![0u8; cap];
        let mut src = input.as_ptr();
        let mut dst = out.as_mut_ptr();
        let r = unsafe {
            llvm_convertutf_utf16_to_utf8(
                &mut src,
                input.as_ptr().add(input.len()),
                &mut dst,
                out.as_mut_ptr().add(cap),
                flags,
            )
        };
        (
            r,
            (src as usize - input.as_ptr() as usize) / 2,
            dst as usize - out.as_ptr() as usize,
            out,
        )
    }

    fn cvt32to8(input: &[u32], flags: u32, cap: usize) -> (u32, usize, usize, Vec<u8>) {
        let mut out = std::vec![0u8; cap];
        let mut src = input.as_ptr();
        let mut dst = out.as_mut_ptr();
        let r = unsafe {
            llvm_convertutf_utf32_to_utf8(
                &mut src,
                input.as_ptr().add(input.len()),
                &mut dst,
                out.as_mut_ptr().add(cap),
                flags,
            )
        };
        (
            r,
            (src as usize - input.as_ptr() as usize) / 4,
            dst as usize - out.as_ptr() as usize,
            out,
        )
    }

    fn cvt16to32(input: &[u16], flags: u32, cap: usize) -> (u32, usize, usize, Vec<u32>) {
        let mut out = std::vec![0u32; cap];
        let mut src = input.as_ptr();
        let mut dst = out.as_mut_ptr();
        let r = unsafe {
            llvm_convertutf_utf16_to_utf32(
                &mut src,
                input.as_ptr().add(input.len()),
                &mut dst,
                out.as_mut_ptr().add(cap),
                flags,
            )
        };
        (
            r,
            (src as usize - input.as_ptr() as usize) / 2,
            (dst as usize - out.as_ptr() as usize) / 4,
            out,
        )
    }

    fn cvt32to16(input: &[u32], flags: u32, cap: usize) -> (u32, usize, usize, Vec<u16>) {
        let mut out = std::vec![0u16; cap];
        let mut src = input.as_ptr();
        let mut dst = out.as_mut_ptr();
        let r = unsafe {
            llvm_convertutf_utf32_to_utf16(
                &mut src,
                input.as_ptr().add(input.len()),
                &mut dst,
                out.as_mut_ptr().add(cap),
                flags,
            )
        };
        (
            r,
            (src as usize - input.as_ptr() as usize) / 4,
            (dst as usize - out.as_ptr() as usize) / 2,
            out,
        )
    }

    const UTF8_SAMPLE: [u8; 11] = [
        0x41, 0xC3, 0xA9, 0xE2, 0x82, 0xAC, 0xF0, 0x9F, 0x98, 0x80, 0x5A,
    ];
    const UTF16_SAMPLE: [u16; 6] = [0x41, 0xE9, 0x20AC, 0xD83D, 0xDE00, 0x5A];
    const UTF32_SAMPLE: [u32; 5] = [0x41, 0xE9, 0x20AC, 0x1F600, 0x5A];

    #[test]
    fn known_conversions() {
        let (r, so, doff, out) = cvt8to16(&UTF8_SAMPLE, STRICT_CONVERSION, 16);
        assert_eq!(r, CONVERSION_OK);
        assert_eq!(so, UTF8_SAMPLE.len());
        assert_eq!(&out[..doff], &UTF16_SAMPLE);

        let (r, so, doff, out) = cvt8to32(&UTF8_SAMPLE, STRICT_CONVERSION, 16);
        assert_eq!(r, CONVERSION_OK);
        assert_eq!(so, UTF8_SAMPLE.len());
        assert_eq!(&out[..doff], &UTF32_SAMPLE);

        let (r, so, doff, out) = cvt16to8(&UTF16_SAMPLE, STRICT_CONVERSION, 32);
        assert_eq!(r, CONVERSION_OK);
        assert_eq!(so, UTF16_SAMPLE.len());
        assert_eq!(&out[..doff], &UTF8_SAMPLE);

        let (r, so, doff, out) = cvt32to8(&UTF32_SAMPLE, STRICT_CONVERSION, 32);
        assert_eq!(r, CONVERSION_OK);
        assert_eq!(so, UTF32_SAMPLE.len());
        assert_eq!(&out[..doff], &UTF8_SAMPLE);

        let (r, so, doff, out) = cvt16to32(&UTF16_SAMPLE, STRICT_CONVERSION, 16);
        assert_eq!(r, CONVERSION_OK);
        assert_eq!(so, UTF16_SAMPLE.len());
        assert_eq!(&out[..doff], &UTF32_SAMPLE);

        let (r, so, doff, out) = cvt32to16(&UTF32_SAMPLE, STRICT_CONVERSION, 16);
        assert_eq!(r, CONVERSION_OK);
        assert_eq!(so, UTF32_SAMPLE.len());
        assert_eq!(&out[..doff], &UTF16_SAMPLE);
    }

    #[test]
    fn ascii_runs() {
        let ascii = [b'x'; 64];
        let (r, so, doff, out) = cvt8to16(&ascii, STRICT_CONVERSION, 128);
        assert_eq!((r, so, doff), (CONVERSION_OK, 64, 64));
        assert!(out[..doff].iter().all(|&c| c == 0x78));

        let (r, so, doff, out) = cvt8to32(&ascii, STRICT_CONVERSION, 128);
        assert_eq!((r, so, doff), (CONVERSION_OK, 64, 64));
        assert!(out[..doff].iter().all(|&c| c == 0x78));

        let wide = [b'y' as u16; 64];
        let (r, so, doff, out) = cvt16to8(&wide, STRICT_CONVERSION, 128);
        assert_eq!((r, so, doff), (CONVERSION_OK, 64, 64));
        assert!(out[..doff].iter().all(|&c| c == b'y'));

        let wide32 = [b'z' as u32; 64];
        let (r, so, doff, out) = cvt32to8(&wide32, STRICT_CONVERSION, 128);
        assert_eq!((r, so, doff), (CONVERSION_OK, 64, 64));
        assert!(out[..doff].iter().all(|&c| c == b'z'));

        let (r, so, doff, out) = cvt16to32(&wide, STRICT_CONVERSION, 128);
        assert_eq!((r, so, doff), (CONVERSION_OK, 64, 64));
        assert!(out[..doff].iter().all(|&c| c == 0x79));

        let (r, so, doff, out) = cvt32to16(&wide32, STRICT_CONVERSION, 128);
        assert_eq!((r, so, doff), (CONVERSION_OK, 64, 64));
        assert!(out[..doff].iter().all(|&c| c == 0x7A));
    }

    #[test]
    fn invalid_sequences() {
        // Overlong NUL: UTF-8 -> UTF-16 rejects without replacement.
        let (r, so, doff, _) = cvt8to16(&[0xC0, 0x80], 1, 8);
        assert_eq!((r, so, doff), (SOURCE_ILLEGAL, 0, 0));
        // UTF-8 -> UTF-32 lenient replaces each byte of the maximal subpart.
        let (r, so, doff, out) = cvt8to32(&[0xC0, 0x80], 1, 8);
        assert_eq!((r, so, doff), (SOURCE_ILLEGAL, 2, 2));
        assert_eq!(&out[..doff], &[UNI_REPLACEMENT_CHAR, UNI_REPLACEMENT_CHAR]);
        // Truncated sequence.
        let (r, so, doff, _) = cvt8to16(&[0xE2, 0x82], 0, 8);
        assert_eq!((r, so, doff), (SOURCE_EXHAUSTED, 0, 0));
        let (r, so, doff, _) = cvt8to32(&[0xE2, 0x82], 0, 8);
        assert_eq!((r, so, doff), (SOURCE_EXHAUSTED, 0, 0));
        let (r, so, doff, out) = cvt8to32(&[0xE2, 0x82], 1, 8);
        assert_eq!((r, so, doff), (SOURCE_ILLEGAL, 2, 1));
        assert_eq!(out[0], UNI_REPLACEMENT_CHAR);
        // Surrogate encoded as UTF-8 is illegal UTF-8.
        let (r, so, doff, _) = cvt8to16(&[0xED, 0xA0, 0x80], 1, 8);
        assert_eq!((r, so, doff), (SOURCE_ILLEGAL, 0, 0));
        // UTF-32 above U+10FFFF.
        let (r, _, doff, out) = cvt32to8(&[0x110000], STRICT_CONVERSION, 8);
        assert_eq!((r, doff), (SOURCE_ILLEGAL, 3));
        assert_eq!(&out[..3], &[0xEF, 0xBF, 0xBD]);
        let (r, _, doff, _) = cvt32to16(&[0x110000], STRICT_CONVERSION, 8);
        assert_eq!((r, doff), (SOURCE_ILLEGAL, 0));
        // Unpaired high surrogate followed by a normal code unit.
        let (r, so, doff, _) = cvt16to8(&[0xD800, 0x41], STRICT_CONVERSION, 8);
        assert_eq!((r, so, doff), (SOURCE_ILLEGAL, 0, 0));
        let (r, so, doff, out) = cvt16to8(&[0xD800, 0x41], 1, 8);
        assert_eq!((r, so, doff), (CONVERSION_OK, 2, 4));
        assert_eq!(&out[..4], &[0xED, 0xA0, 0x80, 0x41]);
    }

    #[test]
    fn target_exhaustion() {
        let (r, so, doff, out) = cvt8to16(&UTF8_SAMPLE, STRICT_CONVERSION, 5);
        assert_eq!((r, so, doff), (TARGET_EXHAUSTED, 10, 5));
        assert_eq!(&out[..5], &UTF16_SAMPLE[..5]);

        let (r, so, doff, _) = cvt8to32(&UTF8_SAMPLE, STRICT_CONVERSION, 4);
        assert_eq!((r, so, doff), (TARGET_EXHAUSTED, 10, 4));

        let (r, so, doff, _) = cvt16to8(&UTF16_SAMPLE, STRICT_CONVERSION, 9);
        assert_eq!((r, so, doff), (TARGET_EXHAUSTED, 3, 6));

        let (r, so, doff, _) = cvt32to16(&UTF32_SAMPLE, STRICT_CONVERSION, 5);
        assert_eq!((r, so, doff), (TARGET_EXHAUSTED, 4, 5));
    }

    #[test]
    fn helpers() {
        unsafe {
            assert_eq!(llvm_convertutf_get_num_bytes_for_utf8(0x00), 1);
            assert_eq!(llvm_convertutf_get_num_bytes_for_utf8(0xC2), 2);
            assert_eq!(llvm_convertutf_get_num_bytes_for_utf8(0xE0), 3);
            assert_eq!(llvm_convertutf_get_num_bytes_for_utf8(0xF0), 4);
            assert_eq!(llvm_convertutf_get_num_bytes_for_utf8(0xFF), 6);
        }

        let legal = UTF8_SAMPLE;
        unsafe {
            assert_eq!(
                llvm_convertutf_is_legal_utf8_sequence(legal.as_ptr(), legal.as_ptr().add(11)),
                1
            );
            assert_eq!(
                llvm_convertutf_get_utf8_sequence_size(legal.as_ptr(), legal.as_ptr().add(11)),
                1
            );
            assert_eq!(
                llvm_convertutf_get_utf8_sequence_size(
                    legal.as_ptr().add(6),
                    legal.as_ptr().add(10)
                ),
                4
            );
            assert_eq!(
                llvm_convertutf_get_utf8_sequence_size(
                    legal.as_ptr().add(6),
                    legal.as_ptr().add(9)
                ),
                0
            );
            let mut p = legal.as_ptr();
            assert_eq!(
                llvm_convertutf_is_legal_utf8_string(&mut p, legal.as_ptr().add(11)),
                1
            );
            assert_eq!(p as usize - legal.as_ptr() as usize, 11);
        }

        let overlong = [0xE0u8, 0x80, 0x80];
        unsafe {
            assert_eq!(
                llvm_convertutf_is_legal_utf8_sequence(overlong.as_ptr(), overlong.as_ptr().add(3)),
                0
            );
            assert_eq!(
                llvm_convertutf_find_maximal_subpart(overlong.as_ptr(), overlong.as_ptr().add(3)),
                1
            );
            let mut p = overlong.as_ptr();
            assert_eq!(
                llvm_convertutf_is_legal_utf8_string(&mut p, overlong.as_ptr().add(3)),
                0
            );
            assert_eq!(p as usize - overlong.as_ptr() as usize, 0);
        }

        let truncated = [0xF0u8, 0x9F, 0x98];
        unsafe {
            assert_eq!(
                llvm_convertutf_find_maximal_subpart(truncated.as_ptr(), truncated.as_ptr().add(3)),
                3
            );
        }
    }
}

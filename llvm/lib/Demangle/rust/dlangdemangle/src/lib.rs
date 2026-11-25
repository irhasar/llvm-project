//===-- lib.rs - Rust port of LLVM's D demangler ----------------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Bit-compatible reimplementation of llvm/lib/Demangle/DLangDemangle.cpp.
// The crate stays no_std; output grows with realloc and the returned pointer
// is freed by the C++ caller with std::free.
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

unsafe extern "C" {
    fn realloc(ptr: *mut core::ffi::c_void, size: usize) -> *mut core::ffi::c_void;
    fn free(ptr: *mut core::ffi::c_void);
}

#[derive(Clone, Copy)]
struct View<'a> {
    data: &'a [u8],
    pos: usize,
    valid: bool,
}

impl<'a> View<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            valid: true,
        }
    }

    fn at(data: &'a [u8], pos: usize) -> Self {
        Self {
            data,
            pos,
            valid: true,
        }
    }

    #[inline(always)]
    fn is_empty(self) -> bool {
        !self.valid || self.pos >= self.data.len()
    }

    #[inline(always)]
    fn len(self) -> usize {
        if self.valid {
            self.data.len() - self.pos
        } else {
            0
        }
    }

    #[inline(always)]
    fn front(self) -> u8 {
        self.data[self.pos]
    }

    #[inline(always)]
    fn byte(self, index: usize) -> u8 {
        self.data[self.pos + index]
    }

    #[inline(always)]
    fn absolute(self) -> usize {
        self.pos
    }

    #[inline(always)]
    fn remove_prefix(&mut self, count: usize) {
        if self.valid {
            self.pos = (self.pos + count).min(self.data.len());
        }
    }

    #[inline(always)]
    fn clear(&mut self) {
        self.valid = false;
        self.pos = 0;
    }

    #[inline(always)]
    fn starts_with(self, prefix: &[u8]) -> bool {
        self.valid
            && self.len() >= prefix.len()
            && &self.data[self.pos..self.pos + prefix.len()] == prefix
    }

    fn from(self, offset: usize) -> Self {
        Self {
            data: self.data,
            pos: (self.pos + offset).min(self.data.len()),
            valid: self.valid,
        }
    }

    fn slice(self, count: usize) -> &'a [u8] {
        &self.data[self.pos..self.pos + count]
    }
}

struct Out {
    buf: *mut u8,
    cap: usize,
    len: usize,
    failed: bool,
}

impl Out {
    fn new(initial_cap: usize) -> Self {
        let mut out = Self {
            buf: core::ptr::null_mut(),
            cap: 0,
            len: 0,
            failed: false,
        };
        if initial_cap != 0 {
            let buf = unsafe { realloc(core::ptr::null_mut(), initial_cap) };
            if buf.is_null() {
                out.failed = true;
            } else {
                out.buf = buf.cast();
                out.cap = initial_cap;
            }
        }
        out
    }

    #[inline(always)]
    fn ensure(&mut self, additional: usize) -> bool {
        if additional <= self.cap.wrapping_sub(self.len) {
            return true;
        }
        self.grow(additional)
    }

    #[cold]
    #[inline(never)]
    fn grow(&mut self, additional: usize) -> bool {
        let need = match self.len.checked_add(additional) {
            Some(need) => need,
            None => {
                self.failed = true;
                return false;
            }
        };

        let mut new_cap = if self.cap == 0 {
            1024
        } else {
            self.cap.saturating_mul(2)
        };
        if self.cap >= 1024 {
            let hysteretic = need.saturating_add(992);
            if new_cap < hysteretic {
                new_cap = hysteretic;
            }
        } else if new_cap < need {
            new_cap = need;
        }
        let new_buf = unsafe { realloc(self.buf.cast(), new_cap) };
        if new_buf.is_null() {
            self.failed = true;
            return false;
        }
        self.buf = new_buf.cast();
        self.cap = new_cap;
        true
    }

    #[inline(always)]
    fn push_byte(&mut self, byte: u8) {
        if !self.ensure(1) {
            return;
        }
        unsafe { *self.buf.add(self.len) = byte };
        self.len += 1;
    }

    #[inline(always)]
    fn push(&mut self, bytes: &[u8]) {
        let n = bytes.len();
        if n == 0 {
            return;
        }
        if !self.ensure(n) {
            return;
        }
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), self.buf.add(self.len), n) };
        self.len += n;
    }

    fn insert(&mut self, pos: usize, bytes: &[u8]) {
        let n = bytes.len();
        if n == 0 {
            return;
        }
        if !self.ensure(n) {
            return;
        }
        let stored = self.len;
        if pos < stored {
            for i in (0..stored - pos).rev() {
                let src = pos + i;
                let value = unsafe { *self.buf.add(src) };
                unsafe { *self.buf.add(src + n) = value };
            }
        }
        for (i, &byte) in bytes.iter().enumerate() {
            unsafe { *self.buf.add(pos + i) = byte };
        }
        self.len += n;
    }

    fn prepend(&mut self, bytes: &[u8]) {
        self.insert(0, bytes);
    }

    fn truncate(&mut self, new_len: usize) {
        self.len = new_len.min(self.len);
    }

    fn as_slice(&self) -> &[u8] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.buf, self.len) }
        }
    }

    fn free(self) {
        if !self.buf.is_null() {
            unsafe { free(self.buf.cast()) };
        }
    }
}

#[inline(always)]
fn is_digit(byte: u8) -> bool {
    byte.is_ascii_digit()
}

#[inline(always)]
fn is_alpha(byte: u8) -> bool {
    byte.is_ascii_alphabetic()
}

fn decode_number(mangled: &mut View<'_>) -> Option<usize> {
    if mangled.is_empty() || !is_digit(mangled.front()) {
        mangled.clear();
        return None;
    }

    let mut value = 0usize;
    loop {
        let digit = (mangled.front() - b'0') as usize;
        if value > (u32::MAX as usize - digit) / 10 {
            mangled.clear();
            return None;
        }
        value = value * 10 + digit;
        mangled.remove_prefix(1);
        if mangled.is_empty() || !is_digit(mangled.front()) {
            break;
        }
    }

    if mangled.is_empty() {
        mangled.clear();
        return None;
    }

    Some(value)
}

fn decode_backref_pos(mangled: &mut View<'_>, ret: &mut usize) -> bool {
    if mangled.is_empty() {
        mangled.clear();
        return false;
    }

    let mut value = 0usize;
    while !mangled.is_empty() && is_alpha(mangled.front()) {
        if value > (usize::MAX - 25) / 26 {
            break;
        }

        value *= 26;
        let byte = mangled.front();
        if byte.is_ascii_lowercase() {
            value += (byte - b'a') as usize;
            if value == 0 {
                break;
            }
            *ret = value;
            mangled.remove_prefix(1);
            return true;
        }

        value += (byte - b'A') as usize;
        mangled.remove_prefix(1);
    }

    mangled.clear();
    false
}

struct Demangler<'a> {
    input: &'a [u8],
    last_backref: usize,
}

impl<'a> Demangler<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            last_backref: input.len(),
        }
    }

    fn decode_backref(&self, mangled: &mut View<'a>, ret: &mut View<'a>) -> bool {
        debug_assert!(!mangled.is_empty() && mangled.front() == b'Q');
        let qpos = mangled.absolute();
        mangled.remove_prefix(1);

        let mut refpos = 0usize;
        if !decode_backref_pos(mangled, &mut refpos) {
            mangled.clear();
            return false;
        }

        if refpos > qpos {
            mangled.clear();
            return false;
        }

        *ret = View::at(self.input, qpos - refpos);
        true
    }

    fn parse_symbol_backref(&mut self, out: &mut Out, mangled: &mut View<'a>) {
        let mut backref = View::at(self.input, 0);
        if !self.decode_backref(mangled, &mut backref) {
            mangled.clear();
            return;
        }

        let len = match decode_number(&mut backref) {
            Some(len) => len,
            None => {
                mangled.clear();
                return;
            }
        };
        if backref.is_empty() || backref.len() < len {
            mangled.clear();
            return;
        }

        self.parse_lname(out, &mut backref, len);
        if backref.is_empty() {
            mangled.clear();
        }
    }

    fn parse_type_backref(&mut self, out: &mut Out, mangled: &mut View<'a>) {
        if mangled.absolute() >= self.last_backref {
            mangled.clear();
            return;
        }

        let saved = self.last_backref;
        self.last_backref = mangled.absolute();

        let mut backref = View::at(self.input, 0);
        if !self.decode_backref(mangled, &mut backref) {
            mangled.clear();
            return;
        }

        if backref.is_empty() {
            mangled.clear();
            return;
        }

        if !self.parse_type(out, &mut backref) {
            mangled.clear();
        }

        self.last_backref = saved;
        if backref.is_empty() {
            mangled.clear();
        }
    }

    fn is_symbol_name(&self, mangled: View<'a>) -> bool {
        if mangled.is_empty() {
            return false;
        }

        let qref = mangled.absolute();
        if is_digit(mangled.front()) {
            return true;
        }
        if mangled.front() != b'Q' {
            return false;
        }

        let mut rest = mangled;
        rest.remove_prefix(1);
        let mut ret = 0usize;
        if !decode_backref_pos(&mut rest, &mut ret) || ret > qref {
            return false;
        }

        is_digit(self.input[qref - ret])
    }

    fn parse_mangle(&mut self, out: &mut Out, mangled: &mut View<'a>) {
        mangled.remove_prefix(2);
        self.parse_qualified(out, mangled);

        if mangled.is_empty() {
            mangled.clear();
            return;
        }

        if mangled.front() == b'Z' {
            mangled.remove_prefix(1);
            return;
        }

        let mut type_out = Out::new(64);
        if !self.parse_type(&mut type_out, mangled) {
            mangled.clear();
        } else {
            let type_len = type_out.len;
            if type_len > 0 {
                out.insert(0, type_out.as_slice());
                out.insert(type_len, b" ");
            }
        }
        type_out.free();
    }

    fn parse_qualified(&mut self, out: &mut Out, mangled: &mut View<'a>) {
        let mut not_first = false;
        loop {
            if !mangled.is_empty() && mangled.front() == b'0' {
                while !mangled.is_empty() && mangled.front() == b'0' {
                    mangled.remove_prefix(1);
                }
                if mangled.is_empty() || !self.is_symbol_name(*mangled) {
                    break;
                }
                continue;
            }

            if not_first {
                out.push_byte(b'.');
            }
            not_first = true;
            self.parse_identifier(out, mangled);

            if mangled.is_empty() || !self.is_symbol_name(*mangled) {
                break;
            }
        }
    }

    fn parse_identifier(&mut self, out: &mut Out, mangled: &mut View<'a>) {
        if mangled.is_empty() {
            mangled.clear();
            return;
        }

        if mangled.front() == b'Q' {
            self.parse_symbol_backref(out, mangled);
            return;
        }

        let len = match decode_number(mangled) {
            Some(len) => len,
            None => {
                mangled.clear();
                return;
            }
        };

        if mangled.is_empty() || len == 0 || mangled.len() < len {
            mangled.clear();
            return;
        }

        if len >= 4 && mangled.starts_with(b"__S") {
            let suffix_len = mangled.len() - len;
            let mut prefix = mangled.from(3);
            while prefix.len() > suffix_len && is_digit(prefix.front()) {
                prefix.remove_prefix(1);
            }
            if prefix.len() == suffix_len {
                mangled.remove_prefix(len);
                self.parse_identifier(out, mangled);
                return;
            }
        }

        self.parse_lname(out, mangled, len);
    }

    fn parse_type(&mut self, out: &mut Out, mangled: &mut View<'a>) -> bool {
        if mangled.is_empty() {
            mangled.clear();
            return false;
        }

        match mangled.front() {
            b'i' => {
                mangled.remove_prefix(1);
                out.push(b"int");
                true
            }
            b'v' => {
                mangled.remove_prefix(1);
                out.push(b"void");
                true
            }
            b'a' => {
                mangled.remove_prefix(1);
                out.push(b"char");
                true
            }
            b'b' => {
                mangled.remove_prefix(1);
                out.push(b"bool");
                true
            }
            b'c' => {
                mangled.remove_prefix(1);
                out.push(b"creal");
                true
            }
            b'd' => {
                mangled.remove_prefix(1);
                out.push(b"double");
                true
            }
            b'e' => {
                mangled.remove_prefix(1);
                out.push(b"real");
                true
            }
            b'f' => {
                mangled.remove_prefix(1);
                out.push(b"float");
                true
            }
            b'g' => {
                mangled.remove_prefix(1);
                out.push(b"byte");
                true
            }
            b'h' => {
                mangled.remove_prefix(1);
                out.push(b"ubyte");
                true
            }
            b'j' => {
                mangled.remove_prefix(1);
                out.push(b"ireal");
                true
            }
            b'k' => {
                mangled.remove_prefix(1);
                out.push(b"uint");
                true
            }
            b'l' => {
                mangled.remove_prefix(1);
                out.push(b"long");
                true
            }
            b'm' => {
                mangled.remove_prefix(1);
                out.push(b"ulong");
                true
            }
            b'n' => {
                mangled.remove_prefix(1);
                true
            }
            b'o' => {
                mangled.remove_prefix(1);
                out.push(b"ifloat");
                true
            }
            b'p' => {
                mangled.remove_prefix(1);
                out.push(b"idouble");
                true
            }
            b'q' => {
                mangled.remove_prefix(1);
                out.push(b"cfloat");
                true
            }
            b'r' => {
                mangled.remove_prefix(1);
                out.push(b"cdouble");
                true
            }
            b's' => {
                mangled.remove_prefix(1);
                out.push(b"short");
                true
            }
            b't' => {
                mangled.remove_prefix(1);
                out.push(b"ushort");
                true
            }
            b'u' => {
                mangled.remove_prefix(1);
                out.push(b"wchar");
                true
            }
            b'w' => {
                mangled.remove_prefix(1);
                out.push(b"dchar");
                true
            }
            b'z' => {
                if mangled.len() < 2 {
                    mangled.clear();
                    return false;
                }
                match mangled.byte(1) {
                    b'i' => {
                        mangled.remove_prefix(2);
                        out.push(b"cent");
                        true
                    }
                    b'k' => {
                        mangled.remove_prefix(2);
                        out.push(b"ucent");
                        true
                    }
                    _ => {
                        mangled.clear();
                        false
                    }
                }
            }
            b'N' => {
                if mangled.len() < 2 {
                    mangled.clear();
                    return false;
                }
                if mangled.byte(1) == b'n' {
                    mangled.remove_prefix(2);
                    out.push(b"noreturn");
                    true
                } else {
                    mangled.clear();
                    false
                }
            }
            b'Q' => {
                self.parse_type_backref(out, mangled);
                true
            }
            _ => {
                mangled.clear();
                false
            }
        }
    }

    fn parse_lname(&self, out: &mut Out, mangled: &mut View<'a>, len: usize) {
        let magic = match len {
            6 if mangled.starts_with(b"__initZ") => Some(b"initializer for ".as_slice()),
            6 if mangled.starts_with(b"__vtblZ") => Some(b"vtable for ".as_slice()),
            7 if mangled.starts_with(b"__ClassZ") => Some(b"ClassInfo for ".as_slice()),
            11 if mangled.starts_with(b"__InterfaceZ") => Some(b"Interface for ".as_slice()),
            12 if mangled.starts_with(b"__ModuleInfoZ") => Some(b"ModuleInfo for ".as_slice()),
            _ => None,
        };
        if let Some(prefix) = magic {
            out.prepend(prefix);
            out.truncate(out.len.saturating_sub(1));
            mangled.remove_prefix(len);
            return;
        }

        out.push(mangled.slice(len));
        mangled.remove_prefix(len);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_dlangdemangle(
    input: *const u8,
    input_len: usize,
    output: *mut *mut u8,
) -> isize {
    if output.is_null() || (input_len != 0 && input.is_null()) {
        return -1;
    }
    *output = core::ptr::null_mut();

    let input = if input_len == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(input, input_len)
    };

    if input.len() < 2 || &input[..2] != b"_D" {
        return -1;
    }

    let mut out = Out::new(64);
    if input == b"_Dmain" {
        out.push(b"D main");
    } else {
        let mut demangler = Demangler::new(input);
        let mut mangled = View::new(input);
        demangler.parse_mangle(&mut out, &mut mangled);
        if !mangled.valid || mangled.absolute() != input.len() {
            out.free();
            return -1;
        }
    }

    if out.failed || out.len == 0 {
        out.free();
        return -1;
    }

    if !out.ensure(1) {
        out.free();
        return -1;
    }
    let len = out.len;
    if len > isize::MAX as usize {
        out.free();
        return -1;
    }

    *output = out.buf;
    len as isize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demangle(input: &[u8]) -> Option<Vec<u8>> {
        let mut output = core::ptr::null_mut();
        let len = unsafe { llvm_dlangdemangle(input.as_ptr(), input.len(), &mut output) };
        if len < 0 {
            return None;
        }
        let result = unsafe { core::slice::from_raw_parts(output, len as usize).to_vec() };
        unsafe { free(output.cast()) };
        Some(result)
    }

    fn check(input: &str, expected: &str) {
        assert_eq!(
            demangle(input.as_bytes()).as_deref(),
            Some(expected.as_bytes())
        );
    }

    #[test]
    fn golden_vectors() {
        check("_Dmain", "D main");
        check("_D3fooi", "int foo");
        check("_D3foo3bari", "int foo.bar");
        check("_D3foo3barZ", "foo.bar");
        check("_D3foo6__initZ", "initializer for foo");
        check("_D3foo6__vtblZ", "vtable for foo");
        check("_D3foo7__ClassZ", "ClassInfo for foo");
        check("_D3foo11__InterfaceZ", "Interface for foo");
        check("_D3foo12__ModuleInfoZ", "ModuleInfo for foo");
        check("_D3foo3barNn", "noreturn foo.bar");
        check("_D3foo3barzi", "cent foo.bar");
        check("_D3foo3barzk", "ucent foo.bar");
        check("_D3foo3barQeZ", "foo.bar.bar");
        check("_D3foo3barQe3bazZ", "foo.bar.bar.baz");
        check("_D4acore", "real acor");
    }

    #[test]
    fn invalid_inputs() {
        for input in [
            "",
            "_D",
            "_D3foo",
            "_D3foobar",
            "_D3foo3barQa",
            "_D3foo3barz",
            "_D3foo3barNx",
            "_D3foo3barX",
            "_Z3foov",
        ] {
            assert_eq!(demangle(input.as_bytes()), None, "{input}");
        }
    }

    #[test]
    fn all_basic_types() {
        let types = [
            (b'i', "int"),
            (b'v', "void"),
            (b'a', "char"),
            (b'b', "bool"),
            (b'c', "creal"),
            (b'd', "double"),
            (b'e', "real"),
            (b'f', "float"),
            (b'g', "byte"),
            (b'h', "ubyte"),
            (b'j', "ireal"),
            (b'k', "uint"),
            (b'l', "long"),
            (b'm', "ulong"),
            (b'o', "ifloat"),
            (b'p', "idouble"),
            (b'q', "cfloat"),
            (b'r', "cdouble"),
            (b's', "short"),
            (b't', "ushort"),
            (b'u', "wchar"),
            (b'w', "dchar"),
        ];
        for (tag, name) in types {
            let input = [b"_D3foo".as_slice(), &[tag]].concat();
            check(
                core::str::from_utf8(&input).unwrap(),
                &format!("{name} foo"),
            );
        }
        check("_D3foon", "foo");
    }
}

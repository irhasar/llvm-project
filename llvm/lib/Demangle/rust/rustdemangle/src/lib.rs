//===-- lib.rs - Rust port of LLVM's Rust v0 demangler ---------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Bit-compatible reimplementation of llvm/lib/Demangle/RustDemangle.cpp.
// The parser follows the Rust v0 mangling RFC. The crate stays no_std; the
// output buffer grows with realloc and the returned pointer is freed by the
// C++ caller with std::free, matching llvm::rustDemangle's contract.
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum IsInType {
    No,
    Yes,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LeaveGenericsOpen {
    No,
    Yes,
}

#[derive(Clone, Copy)]
enum BasicType {
    Bool,
    Char,
    I8,
    I16,
    I32,
    I64,
    I128,
    ISize,
    U8,
    U16,
    U32,
    U64,
    U128,
    USize,
    F32,
    F64,
    Str,
    Placeholder,
    Unit,
    Variadic,
    Never,
}

#[derive(Clone, Copy)]
struct Identifier {
    start: usize,
    end: usize,
    punycode: bool,
}

impl Identifier {
    fn empty() -> Self {
        Self {
            start: 0,
            end: 0,
            punycode: false,
        }
    }

    fn is_empty(self) -> bool {
        self.start == self.end
    }
}

struct Out {
    buf: *mut u8,
    cap: usize,
    len: usize,
    failed: bool,
}

impl Out {
    fn new() -> Self {
        Self {
            buf: core::ptr::null_mut(),
            cap: 0,
            len: 0,
            failed: false,
        }
    }

    #[inline(always)]
    fn ensure(&mut self, additional: usize) -> bool {
        // len <= cap is an invariant, so this is a single subtract and compare.
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
        // Match OutputBuffer's hysteresis so long outputs do not reallocate at
        // every doubling boundary.
        let hysteretic = need.saturating_add(992);
        if new_cap < hysteretic {
            new_cap = hysteretic;
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

    #[inline(always)]
    fn write_u64(&mut self, mut value: u64) {
        let mut digits = [0u8; 20];
        let mut pos = digits.len();
        loop {
            pos -= 1;
            digits[pos] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        self.push(&digits[pos..]);
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
        if pos < stored && n != 0 {
            for i in (0..stored - pos).rev() {
                let src = pos + i;
                let dst = src + n;
                let value = unsafe { *self.buf.add(src) };
                unsafe { *self.buf.add(dst) = value };
            }
        }
        for (i, &byte) in bytes.iter().enumerate() {
            unsafe { *self.buf.add(pos + i) = byte };
        }
        self.len += n;
    }

    fn remove_null_bytes(&mut self, start: usize) {
        let stored = self.len;
        let start = start.min(stored);
        let mut write = start;
        for read in start..stored {
            let value = unsafe { *self.buf.add(read) };
            if value != 0 {
                if write != read {
                    unsafe { *self.buf.add(write) = value };
                }
                write += 1;
            }
        }
        self.len -= stored - write;
    }
}

struct Demangler<'a> {
    max_recursion_level: usize,
    recursion_level: usize,
    bound_lifetimes: usize,
    input: &'a [u8],
    position: usize,
    print: bool,
    error: bool,
    out: Out,
}

impl<'a> Demangler<'a> {
    #[cold]
    #[inline(never)]
    fn fail(&mut self) {
        self.error = true;
    }

    fn new(out: Out) -> Self {
        Self {
            max_recursion_level: 500,
            recursion_level: 0,
            bound_lifetimes: 0,
            input: &[],
            position: 0,
            print: true,
            error: false,
            out,
        }
    }

    fn demangle(&mut self, mangled: &'a [u8]) -> bool {
        self.position = 0;
        self.error = false;
        self.print = true;
        self.recursion_level = 0;
        self.bound_lifetimes = 0;

        if !mangled.starts_with(b"_R") {
            self.fail();
            return false;
        }

        let rest = &mangled[2..];
        let dot = rest.iter().position(|&byte| byte == b'.');
        let (input, suffix) = match dot {
            Some(index) => (&rest[..index], Some(&rest[index..])),
            None => (rest, None),
        };
        self.input = input;

        self.demangle_path(IsInType::No, LeaveGenericsOpen::No);

        if self.position != self.input.len() {
            let saved_print = self.print;
            self.print = false;
            self.demangle_path(IsInType::No, LeaveGenericsOpen::No);
            self.print = saved_print;
        }

        if self.position != self.input.len() {
            self.fail();
        }

        if let Some(suffix) = suffix {
            self.print_str(b" (");
            self.print_str(suffix);
            self.print_char(b')');
        }

        !self.error
    }

    #[inline(always)]
    fn input_slice(&self, start: usize, end: usize) -> &'a [u8] {
        let input = self.input;
        unsafe { input.get_unchecked(start..end) }
    }

    fn demangle_path(&mut self, in_type: IsInType, leave_open: LeaveGenericsOpen) -> bool {
        if self.error || self.recursion_level >= self.max_recursion_level {
            self.fail();
            return false;
        }
        self.recursion_level += 1;
        let result = self.demangle_path_body(in_type, leave_open);
        self.recursion_level -= 1;
        result
    }

    fn demangle_path_body(&mut self, in_type: IsInType, leave_open: LeaveGenericsOpen) -> bool {
        match self.consume() {
            b'C' => {
                self.parse_optional_base62_number(b's');
                let ident = self.parse_identifier();
                self.print_identifier(ident);
            }
            b'M' => {
                self.demangle_impl_path(in_type);
                self.print_char(b'<');
                self.demangle_type();
                self.print_char(b'>');
            }
            b'X' => {
                self.demangle_impl_path(in_type);
                self.print_char(b'<');
                self.demangle_type();
                self.print_str(b" as ");
                self.demangle_path(IsInType::Yes, LeaveGenericsOpen::No);
                self.print_char(b'>');
            }
            b'Y' => {
                self.print_char(b'<');
                self.demangle_type();
                self.print_str(b" as ");
                self.demangle_path(IsInType::Yes, LeaveGenericsOpen::No);
                self.print_char(b'>');
            }
            b'N' => {
                let ns = self.consume();
                if !is_lower(ns) && !is_upper(ns) {
                    self.fail();
                    return false;
                }
                self.demangle_path(in_type, LeaveGenericsOpen::No);
                let disambiguator = self.parse_optional_base62_number(b's');
                let ident = self.parse_identifier();
                if is_upper(ns) {
                    self.print_str(b"::{");
                    if ns == b'C' {
                        self.print_str(b"closure");
                    } else if ns == b'S' {
                        self.print_str(b"shim");
                    } else {
                        self.print_char(ns);
                    }
                    if !ident.is_empty() {
                        self.print_char(b':');
                        self.print_identifier(ident);
                    }
                    self.print_char(b'#');
                    self.print_decimal_number(disambiguator);
                    self.print_char(b'}');
                } else if !ident.is_empty() {
                    self.print_str(b"::");
                    self.print_identifier(ident);
                }
            }
            b'I' => {
                self.demangle_path(in_type, LeaveGenericsOpen::No);
                if in_type == IsInType::No {
                    self.print_str(b"::");
                }
                self.print_char(b'<');
                let mut index = 0;
                while !self.error && !self.consume_if(b'E') {
                    if index > 0 {
                        self.print_str(b", ");
                    }
                    self.demangle_generic_arg();
                    index += 1;
                }
                if leave_open == LeaveGenericsOpen::Yes {
                    return true;
                }
                self.print_char(b'>');
            }
            b'B' => {
                let mut is_open = false;
                self.demangle_backref(|demangler| {
                    is_open = demangler.demangle_path(in_type, leave_open);
                });
                return is_open;
            }
            _ => {
                self.fail();
            }
        }
        false
    }

    #[inline]
    fn demangle_impl_path(&mut self, in_type: IsInType) {
        let saved_print = self.print;
        self.print = false;
        self.parse_optional_base62_number(b's');
        self.demangle_path(in_type, LeaveGenericsOpen::No);
        self.print = saved_print;
    }

    #[inline]
    fn demangle_generic_arg(&mut self) {
        if self.consume_if(b'L') {
            let lifetime = self.parse_base62_number();
            self.print_lifetime(lifetime);
        } else if self.consume_if(b'K') {
            self.demangle_const();
        } else {
            self.demangle_type();
        }
    }

    fn demangle_type(&mut self) {
        if self.error || self.recursion_level >= self.max_recursion_level {
            self.fail();
            return;
        }
        self.recursion_level += 1;
        self.demangle_type_body();
        self.recursion_level -= 1;
    }

    fn demangle_type_body(&mut self) {
        let start = self.position;
        let code = self.consume();
        if let Some(basic) = parse_basic_type(code) {
            self.print_basic_type(basic);
            return;
        }

        match code {
            b'A' => {
                self.print_char(b'[');
                self.demangle_type();
                self.print_str(b"; ");
                self.demangle_const();
                self.print_char(b']');
            }
            b'S' => {
                self.print_char(b'[');
                self.demangle_type();
                self.print_char(b']');
            }
            b'T' => {
                self.print_char(b'(');
                let mut count = 0;
                while !self.error && !self.consume_if(b'E') {
                    if count > 0 {
                        self.print_str(b", ");
                    }
                    self.demangle_type();
                    count += 1;
                }
                if count == 1 {
                    self.print_char(b',');
                }
                self.print_char(b')');
            }
            b'R' | b'Q' => {
                self.print_char(b'&');
                if self.consume_if(b'L') {
                    let lifetime = self.parse_base62_number();
                    if lifetime != 0 {
                        self.print_lifetime(lifetime);
                        self.print_char(b' ');
                    }
                }
                if code == b'Q' {
                    self.print_str(b"mut ");
                }
                self.demangle_type();
            }
            b'P' => {
                self.print_str(b"*const ");
                self.demangle_type();
            }
            b'O' => {
                self.print_str(b"*mut ");
                self.demangle_type();
            }
            b'F' => self.demangle_fn_sig(),
            b'D' => {
                self.demangle_dyn_bounds();
                if self.consume_if(b'L') {
                    let lifetime = self.parse_base62_number();
                    if lifetime != 0 {
                        self.print_str(b" + ");
                        self.print_lifetime(lifetime);
                    }
                } else {
                    self.fail();
                }
            }
            b'B' => self.demangle_backref(|demangler| demangler.demangle_type()),
            _ => {
                self.position = start;
                self.demangle_path(IsInType::Yes, LeaveGenericsOpen::No);
            }
        }
    }

    #[inline(never)]
    fn demangle_fn_sig(&mut self) {
        let saved_bound_lifetimes = self.bound_lifetimes;
        self.demangle_optional_binder();

        if self.consume_if(b'U') {
            self.print_str(b"unsafe ");
        }

        if self.consume_if(b'K') {
            self.print_str(b"extern \"");
            if self.consume_if(b'C') {
                self.print_str(b"C");
            } else {
                let ident = self.parse_identifier();
                if ident.punycode {
                    self.fail();
                }
                let name = self.input_slice(ident.start, ident.end);
                for &byte in name {
                    let byte = if byte == b'_' { b'-' } else { byte };
                    self.print_char(byte);
                }
            }
            self.print_str(b"\" ");
        }

        self.print_str(b"fn(");
        let mut count = 0;
        while !self.error && !self.consume_if(b'E') {
            if count > 0 {
                self.print_str(b", ");
            }
            self.demangle_type();
            count += 1;
        }
        self.print_char(b')');

        if !self.consume_if(b'u') {
            self.print_str(b" -> ");
            self.demangle_type();
        }

        self.bound_lifetimes = saved_bound_lifetimes;
    }

    #[inline(never)]
    fn demangle_dyn_bounds(&mut self) {
        let saved_bound_lifetimes = self.bound_lifetimes;
        self.print_str(b"dyn ");
        self.demangle_optional_binder();
        let mut count = 0;
        while !self.error && !self.consume_if(b'E') {
            if count > 0 {
                self.print_str(b" + ");
            }
            self.demangle_dyn_trait();
            count += 1;
        }
        self.bound_lifetimes = saved_bound_lifetimes;
    }

    #[inline(never)]
    fn demangle_dyn_trait(&mut self) {
        let mut is_open = self.demangle_path(IsInType::Yes, LeaveGenericsOpen::Yes);
        while !self.error && self.consume_if(b'p') {
            if !is_open {
                is_open = true;
                self.print_char(b'<');
            } else {
                self.print_str(b", ");
            }
            let ident = self.parse_identifier();
            let name = self.input_slice(ident.start, ident.end);
            self.print_str(name);
            self.print_str(b" = ");
            self.demangle_type();
        }
        if is_open {
            self.print_char(b'>');
        }
    }

    fn demangle_optional_binder(&mut self) {
        let binder = self.parse_optional_base62_number(b'G');
        if self.error || binder == 0 {
            return;
        }

        let remaining = self.input.len().wrapping_sub(self.bound_lifetimes);
        if binder >= remaining as u64 {
            self.fail();
            return;
        }

        self.print_str(b"for<");
        let mut index = 0;
        while index != binder {
            self.bound_lifetimes += 1;
            if index > 0 {
                self.print_str(b", ");
            }
            self.print_lifetime(1);
            index += 1;
        }
        self.print_str(b"> ");
    }

    fn demangle_const(&mut self) {
        if self.error || self.recursion_level >= self.max_recursion_level {
            self.fail();
            return;
        }
        self.recursion_level += 1;
        self.demangle_const_body();
        self.recursion_level -= 1;
    }

    fn demangle_const_body(&mut self) {
        let code = self.consume();
        if let Some(basic) = parse_basic_type(code) {
            match basic {
                BasicType::I8
                | BasicType::I16
                | BasicType::I32
                | BasicType::I64
                | BasicType::I128
                | BasicType::ISize
                | BasicType::U8
                | BasicType::U16
                | BasicType::U32
                | BasicType::U64
                | BasicType::U128
                | BasicType::USize => self.demangle_const_int(),
                BasicType::Bool => self.demangle_const_bool(),
                BasicType::Char => self.demangle_const_char(),
                BasicType::Placeholder => self.print_char(b'_'),
                _ => self.error = true,
            }
        } else if code == b'B' {
            self.demangle_backref(|demangler| demangler.demangle_const());
        } else {
            self.fail();
        }
    }

    fn demangle_const_int(&mut self) {
        if self.consume_if(b'n') {
            self.print_char(b'-');
        }
        let (value, start, end) = self.parse_hex_number();
        if end - start <= 16 {
            self.print_decimal_number(value);
        } else {
            self.print_str(b"0x");
            let digits = self.input_slice(start, end);
            self.print_str(digits);
        }
    }

    fn demangle_const_bool(&mut self) {
        let (_, start, end) = self.parse_hex_number();
        let digits = self.input_slice(start, end);
        if digits == b"0" {
            self.print_str(b"false");
        } else if digits == b"1" {
            self.print_str(b"true");
        } else {
            self.fail();
        }
    }

    #[inline(never)]
    fn demangle_const_char(&mut self) {
        let (code_point, start, end) = self.parse_hex_number();
        if self.error || end - start > 6 {
            self.fail();
            return;
        }

        self.print_char(b'\'');
        match code_point {
            0x09 => self.print_str(b"\\t"),
            0x0d => self.print_str(b"\\r"),
            0x0a => self.print_str(b"\\n"),
            0x5c => self.print_str(b"\\\\"),
            0x22 => self.print_str(b"\""),
            0x27 => self.print_str(b"\\'"),
            _ if is_ascii_printable(code_point) => self.print_char(code_point as u8),
            _ => {
                self.print_str(b"\\u{");
                let digits = self.input_slice(start, end);
                self.print_str(digits);
                self.print_char(b'}');
            }
        }
        self.print_char(b'\'');
    }

    #[inline]
    fn parse_identifier(&mut self) -> Identifier {
        let punycode = self.consume_if(b'u');
        let bytes = self.parse_decimal_number();
        self.consume_if(b'_');

        if self.error
            || self.position > self.input.len()
            || bytes > (self.input.len() - self.position) as u64
        {
            self.fail();
            return Identifier::empty();
        }

        let start = self.position;
        self.position += bytes as usize;
        let end = self.position;

        let bytes = unsafe { self.input.get_unchecked(start..end) };
        if !bytes.iter().all(|&byte| is_valid(byte)) {
            self.fail();
            return Identifier::empty();
        }

        Identifier {
            start,
            end,
            punycode,
        }
    }

    #[inline]
    fn parse_optional_base62_number(&mut self, tag: u8) -> u64 {
        if !self.consume_if(tag) {
            return 0;
        }

        let mut value = self.parse_base62_number();
        if self.error || !self.add_assign(&mut value, 1) {
            return 0;
        }
        value
    }

    #[inline]
    fn parse_base62_number(&mut self) -> u64 {
        if self.consume_if(b'_') {
            return 0;
        }

        let mut value = 0u64;
        loop {
            let code = self.consume();
            let digit = if code == b'_' {
                break;
            } else {
                let digit = BASE62_DIGITS[code as usize];
                if digit == 0xff {
                    self.fail();
                    return 0;
                }
                digit as u64
            };

            if !self.mul_assign(&mut value, 62) || !self.add_assign(&mut value, digit) {
                return 0;
            }
        }

        if !self.add_assign(&mut value, 1) {
            return 0;
        }
        value
    }

    #[inline]
    fn parse_decimal_number(&mut self) -> u64 {
        let code = self.look();
        if !is_digit(code) {
            self.fail();
            return 0;
        }

        if code == b'0' {
            self.consume();
            return 0;
        }

        let mut value = 0u64;
        while is_digit(self.look()) {
            if !self.mul_assign(&mut value, 10) {
                self.fail();
                return 0;
            }
            let digit = (self.consume() - b'0') as u64;
            if !self.add_assign(&mut value, digit) {
                return 0;
            }
        }
        value
    }

    #[inline]
    fn parse_hex_number(&mut self) -> (u64, usize, usize) {
        let start = self.position;
        let mut value = 0u64;

        if !is_hex_digit(self.look()) {
            self.fail();
        }

        if self.consume_if(b'0') {
            if !self.consume_if(b'_') {
                self.fail();
            }
        } else {
            while !self.error && !self.consume_if(b'_') {
                let code = self.consume();
                value = value.wrapping_mul(16);
                let digit = HEX_DIGITS[code as usize];
                if digit == 0xff {
                    self.fail();
                } else {
                    value = value.wrapping_add(digit as u64);
                }
            }
        }

        if self.error {
            return (0, 0, 0);
        }

        let end = self.position - 1;
        (value, start, end)
    }

    #[inline(always)]
    fn print_char(&mut self, byte: u8) {
        if self.error || !self.print {
            return;
        }
        self.out.push_byte(byte);
    }

    #[inline(always)]
    fn print_str(&mut self, bytes: &[u8]) {
        if self.error || !self.print {
            return;
        }
        self.out.push(bytes);
    }

    #[inline(always)]
    fn print_decimal_number(&mut self, value: u64) {
        if self.error || !self.print {
            return;
        }
        self.out.write_u64(value);
    }

    fn print_lifetime(&mut self, index: u64) {
        if index == 0 {
            self.print_str(b"'_");
            return;
        }

        if index > self.bound_lifetimes as u64 {
            self.fail();
            return;
        }

        let depth = self.bound_lifetimes as u64 - index;
        self.print_char(b'\'');
        if depth < 26 {
            self.print_char(b'a' + depth as u8);
        } else {
            self.print_char(b'z');
            self.print_decimal_number(depth - 26 + 1);
        }
    }

    #[inline(always)]
    fn print_identifier(&mut self, ident: Identifier) {
        if self.error || !self.print {
            return;
        }

        let name = self.input_slice(ident.start, ident.end);
        if ident.punycode {
            self.print_punycode(name);
        } else {
            self.out.push(name);
        }
    }

    #[inline(never)]
    fn print_punycode(&mut self, name: &[u8]) {
        if !decode_punycode(name, &mut self.out) {
            self.fail();
        }
    }

    #[inline]
    fn print_basic_type(&mut self, basic: BasicType) {
        match basic {
            BasicType::Bool => self.print_str(b"bool"),
            BasicType::Char => self.print_str(b"char"),
            BasicType::I8 => self.print_str(b"i8"),
            BasicType::I16 => self.print_str(b"i16"),
            BasicType::I32 => self.print_str(b"i32"),
            BasicType::I64 => self.print_str(b"i64"),
            BasicType::I128 => self.print_str(b"i128"),
            BasicType::ISize => self.print_str(b"isize"),
            BasicType::U8 => self.print_str(b"u8"),
            BasicType::U16 => self.print_str(b"u16"),
            BasicType::U32 => self.print_str(b"u32"),
            BasicType::U64 => self.print_str(b"u64"),
            BasicType::U128 => self.print_str(b"u128"),
            BasicType::USize => self.print_str(b"usize"),
            BasicType::F32 => self.print_str(b"f32"),
            BasicType::F64 => self.print_str(b"f64"),
            BasicType::Str => self.print_str(b"str"),
            BasicType::Placeholder => self.print_char(b'_'),
            BasicType::Unit => self.print_str(b"()"),
            BasicType::Variadic => self.print_str(b"..."),
            BasicType::Never => self.print_char(b'!'),
        }
    }

    #[inline(always)]
    fn look(&self) -> u8 {
        if self.error || self.position >= self.input.len() {
            0
        } else {
            self.input[self.position]
        }
    }

    #[inline(always)]
    fn consume(&mut self) -> u8 {
        if self.error || self.position >= self.input.len() {
            self.fail();
            return 0;
        }
        let byte = self.input[self.position];
        self.position += 1;
        byte
    }

    #[inline(always)]
    fn consume_if(&mut self, prefix: u8) -> bool {
        if self.error || self.position >= self.input.len() || self.input[self.position] != prefix {
            return false;
        }
        self.position += 1;
        true
    }

    fn demangle_backref<F: FnOnce(&mut Self)>(&mut self, f: F) {
        let backref = self.parse_base62_number();
        if self.error || backref >= self.position as u64 {
            self.fail();
            return;
        }
        if !self.print {
            return;
        }

        let saved_position = self.position;
        self.position = backref as usize;
        f(self);
        self.position = saved_position;
    }

    fn add_assign(&mut self, value: &mut u64, addend: u64) -> bool {
        if *value > u64::MAX - addend {
            self.fail();
            return false;
        }
        *value += addend;
        true
    }

    fn mul_assign(&mut self, value: &mut u64, factor: u64) -> bool {
        if factor != 0 && *value > u64::MAX / factor {
            self.fail();
            return false;
        }
        *value *= factor;
        true
    }
}

const fn make_char_class() -> [u8; 256] {
    let mut table = [0u8; 256];
    let mut index = 0;
    while index < 256 {
        let byte = index as u8;
        table[index] = if byte >= b'0' && byte <= b'9' {
            1
        } else if byte >= b'a' && byte <= b'z' {
            2
        } else if byte >= b'A' && byte <= b'Z' {
            3
        } else if byte == b'_' {
            4
        } else {
            0
        };
        index += 1;
    }
    table
}

const fn make_base62_digits() -> [u8; 256] {
    let mut table = [0xffu8; 256];
    let mut index = 0;
    while index < 256 {
        let byte = index as u8;
        table[index] = if byte >= b'0' && byte <= b'9' {
            byte - b'0'
        } else if byte >= b'a' && byte <= b'z' {
            10 + (byte - b'a')
        } else if byte >= b'A' && byte <= b'Z' {
            36 + (byte - b'A')
        } else {
            0xff
        };
        index += 1;
    }
    table
}

const fn make_hex_digits() -> [u8; 256] {
    let mut table = [0xffu8; 256];
    let mut index = 0;
    while index < 256 {
        let byte = index as u8;
        table[index] = if byte >= b'0' && byte <= b'9' {
            byte - b'0'
        } else if byte >= b'a' && byte <= b'f' {
            10 + (byte - b'a')
        } else {
            0xff
        };
        index += 1;
    }
    table
}

static CHAR_CLASS: [u8; 256] = make_char_class();
static BASE62_DIGITS: [u8; 256] = make_base62_digits();
static HEX_DIGITS: [u8; 256] = make_hex_digits();

#[inline(always)]
fn is_digit(byte: u8) -> bool {
    CHAR_CLASS[byte as usize] == 1
}

#[inline(always)]
fn is_hex_digit(byte: u8) -> bool {
    HEX_DIGITS[byte as usize] != 0xff
}

#[inline(always)]
fn is_lower(byte: u8) -> bool {
    CHAR_CLASS[byte as usize] == 2
}

#[inline(always)]
fn is_upper(byte: u8) -> bool {
    CHAR_CLASS[byte as usize] == 3
}

#[inline(always)]
fn is_valid(byte: u8) -> bool {
    CHAR_CLASS[byte as usize] != 0
}

fn parse_basic_type(byte: u8) -> Option<BasicType> {
    Some(match byte {
        b'a' => BasicType::I8,
        b'b' => BasicType::Bool,
        b'c' => BasicType::Char,
        b'd' => BasicType::F64,
        b'e' => BasicType::Str,
        b'f' => BasicType::F32,
        b'h' => BasicType::U8,
        b'i' => BasicType::ISize,
        b'j' => BasicType::USize,
        b'l' => BasicType::I32,
        b'm' => BasicType::U32,
        b'n' => BasicType::I128,
        b'o' => BasicType::U128,
        b'p' => BasicType::Placeholder,
        b's' => BasicType::I16,
        b't' => BasicType::U16,
        b'u' => BasicType::Unit,
        b'v' => BasicType::Variadic,
        b'x' => BasicType::I64,
        b'y' => BasicType::U64,
        b'z' => BasicType::Never,
        _ => return None,
    })
}

fn is_ascii_printable(code_point: u64) -> bool {
    (0x20..=0x7e).contains(&code_point)
}

fn decode_punycode_digit(byte: u8) -> Option<usize> {
    if is_lower(byte) {
        Some((byte - b'a') as usize)
    } else if is_digit(byte) {
        Some(26 + (byte - b'0') as usize)
    } else {
        None
    }
}

fn adapt(mut delta: usize, num_points: usize, damp: &mut usize) -> usize {
    delta /= *damp;
    delta += delta / num_points;
    *damp = 2;

    let mut k = 0usize;
    while delta > 455 {
        delta /= 35;
        k = k.wrapping_add(36);
    }
    k.wrapping_add(36usize.wrapping_mul(delta) / delta.wrapping_add(38))
}

fn encode_utf8(code_point: usize, out: &mut [u8; 4]) -> bool {
    if (0xd800..=0xdfff).contains(&code_point) {
        return false;
    }

    if code_point <= 0x7f {
        out[0] = code_point as u8;
        return true;
    }
    if code_point <= 0x7ff {
        out[0] = 0xc0 | ((code_point >> 6) & 0x3f) as u8;
        out[1] = 0x80 | (code_point & 0x3f) as u8;
        return true;
    }
    if code_point <= 0xffff {
        out[0] = 0xe0 | (code_point >> 12) as u8;
        out[1] = 0x80 | ((code_point >> 6) & 0x3f) as u8;
        out[2] = 0x80 | (code_point & 0x3f) as u8;
        return true;
    }
    if code_point <= 0x10ffff {
        out[0] = 0xf0 | (code_point >> 18) as u8;
        out[1] = 0x80 | ((code_point >> 12) & 0x3f) as u8;
        out[2] = 0x80 | ((code_point >> 6) & 0x3f) as u8;
        out[3] = 0x80 | (code_point & 0x3f) as u8;
        return true;
    }
    false
}

#[inline(never)]
fn decode_punycode(input: &[u8], out: &mut Out) -> bool {
    let output_size = out.len;
    let mut input_index = 0usize;

    let mut delimiter_position = None;
    for (index, &byte) in input.iter().enumerate() {
        if byte == b'_' {
            delimiter_position = Some(index);
        }
    }

    if let Some(delimiter) = delimiter_position {
        while input_index != delimiter {
            let byte = input[input_index];
            if !is_valid(byte) {
                return false;
            }
            out.push_byte(byte);
            out.push_byte(0);
            out.push_byte(0);
            out.push_byte(0);
            input_index += 1;
        }
        input_index += 1;
    }

    let base = 36usize;
    let mut bias = 72usize;
    let mut n = 0x80usize;
    let t_min = 1usize;
    let t_max = 26usize;
    let mut damp = 700usize;
    let max = usize::MAX;

    let mut i = 0usize;
    while input_index != input.len() {
        let old_i = i;
        let mut w = 1usize;
        let mut k = base;

        loop {
            if input_index == input.len() {
                return false;
            }
            let byte = input[input_index];
            input_index += 1;
            let digit = match decode_punycode_digit(byte) {
                Some(digit) => digit,
                None => return false,
            };

            if digit > (max - i) / w {
                return false;
            }
            i = i.wrapping_add(digit.wrapping_mul(w));

            let t = if k <= bias {
                t_min
            } else if k >= bias + t_max {
                t_max
            } else {
                k - bias
            };

            if digit < t {
                break;
            }

            if w > max / (base - t) {
                return false;
            }
            w *= base - t;
            k = k.wrapping_add(base);
        }

        let num_points = (out.len - output_size) / 4 + 1;
        bias = adapt(i - old_i, num_points, &mut damp);

        if i / num_points > max - n {
            return false;
        }
        n += i / num_points;
        i %= num_points;

        let mut utf8 = [0u8; 4];
        if !encode_utf8(n, &mut utf8) {
            return false;
        }
        out.insert(output_size + i * 4, &utf8);
        i += 1;
    }

    out.remove_null_bytes(output_size);
    true
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_rustdemangle_demangle(
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
    let mut demangler = Demangler::new(Out::new());
    if !demangler.demangle(input) || demangler.out.failed {
        if !demangler.out.buf.is_null() {
            free(demangler.out.buf.cast());
        }
        return -1;
    }

    // Reserve the terminator before returning the buffer to C++.
    demangler.out.push_byte(0);
    if demangler.out.failed {
        if !demangler.out.buf.is_null() {
            free(demangler.out.buf.cast());
        }
        return -1;
    }
    let len = demangler.out.len - 1;
    if len > isize::MAX as usize {
        free(demangler.out.buf.cast());
        return -1;
    }

    *output = demangler.out.buf;
    len as isize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demangle(input: &[u8]) -> Option<Vec<u8>> {
        let mut output = core::ptr::null_mut();
        let len = unsafe { llvm_rustdemangle_demangle(input.as_ptr(), input.len(), &mut output) };
        if len < 0 {
            return None;
        }
        let result = unsafe { core::slice::from_raw_parts(output, len as usize) }.to_vec();
        unsafe { free(output.cast()) };
        Some(result)
    }

    #[test]
    fn rejects_non_rust_symbols() {
        assert!(demangle(b"").is_none());
        assert!(demangle(b"_Z3foov").is_none());
        assert!(demangle(b"_R").is_none());
    }

    #[test]
    fn simple_crate_and_function() {
        let output = demangle(b"_RNvC6_123foo3bar").expect("demangles");
        assert_eq!(output, b"123foo::bar");
    }

    #[test]
    fn golden_vectors() {
        let cases: &[(&[u8], &[u8])] = &[
            (
                b"_RNvNtNtCs8uuN1nrGffB_4core3str5count14do_count_chars",
                b"core::str::count::do_count_chars",
            ),
            (
                b"_RINvMNtCs8uuN1nrGffB_4core3stre12trim_matchesNvMNtNtB5_4char7methodsc13is_whitespaceECsh0g7Hz38UNf_3std",
                b"<str>::trim_matches::<<char>::is_whitespace>",
            ),
            (
                b"_RNvXsR_NtCs8uuN1nrGffB_4core6optionINtB5_6OptionmENtNtB7_3fmt5Debug3fmtCsh0g7Hz38UNf_3std",
                b"<core::option::Option<u32> as core::fmt::Debug>::fmt",
            ),
        ];
        for (input, expected) in cases {
            let output = demangle(input).expect("demangles");
            assert_eq!(&output, expected);
        }
    }

    #[test]
    fn punycode_and_suffix() {
        let output = demangle(b"_RCu9_bcher_kva").expect("demangles");
        assert_eq!(output, "bücher".as_bytes());

        let output = demangle(b"_RNvC6_123foo3bar.llvm.1234").expect("demangles");
        assert_eq!(output, b"123foo::bar (.llvm.1234)");
    }
}

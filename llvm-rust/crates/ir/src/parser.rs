//! Textual LLVM IR parser for the first Track B slice.
//!
//! Supported: module headers, named types, globals, declarations,
//! definitions, integer/float/aggregate constants, the core instruction set,
//! and raw attribute spellings. Metadata is accepted and skipped; MC, debug
//! info, and constant expressions are later milestones.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::sync::Arc;

use llvm_rust_support::{Error, FastHashMap, Result, Span, Symbol};

use crate::constant::{ConstExpr, ConstId, ConstKind, Constant};
use crate::function::{BlockId, Function, LocalId};
use crate::inst::{
    AtomicOrdering, AtomicRmwOp, Attribute, BinOp, CastOp, FastMath, FcmpPred, GepIndex, IcmpPred,
    InstKind, Instruction, IntFlags, LandingPadClause, SyncScope, TailKind,
};
use crate::linkage::{Linkage, Visibility};
use crate::module::{Global, Module, ThreadLocal};
use crate::names::quote_bytes;
use crate::ty::{FloatKind, INVALID_TYPE, Type, TypeId};
use crate::value::{Operand, OperandKind};

/// Parser error. Kept as an alias so callers can use the shared error type.
pub type ParseError = Error;

/// Parses one textual IR module.
pub fn parse_module(source: &str) -> core::result::Result<Module, ParseError> {
    let mut parser = Parser::new(Lexer::new(source));
    parser.ensure(2);
    parser.parse_top_level()?;
    if let Some(error) = parser.lex_error {
        return Err(error);
    }
    parser.module.resolve_forward_references();
    parser.module.resolve_forward_operands();
    parser.module.assign_unnamed_slots();
    Ok(parser.module)
}

#[derive(Clone, Debug, PartialEq)]
enum TokenKind<'a> {
    Local(Cow<'a, str>),
    Global(Cow<'a, str>),
    Meta(Cow<'a, str>),
    AttrGroup(u32),
    Ident(Cow<'a, str>),
    Int(u128),
    Float(Cow<'a, str>),
    Str(Vec<u8>),
    CStr(Vec<u8>),
    Punct(char),
    Ellipsis,
    Eol,
    Eof,
}

#[derive(Clone, Debug)]
struct Token<'a> {
    kind: TokenKind<'a>,
    span: Span,
    quoted: bool,
}

struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
        }
    }

    #[cfg(test)]
    fn tokenize(mut self) -> Result<Vec<Token<'a>>> {
        let mut tokens = Vec::with_capacity(self.bytes.len() / 8);
        while let Some(token) = self.next_token()? {
            tokens.push(token);
        }
        tokens.push(self.eof_token());
        Ok(tokens)
    }

    fn next_token(&mut self) -> Result<Option<Token<'a>>> {
        while self.pos < self.bytes.len() {
            let start = self.pos;
            let byte = self.bytes[self.pos];
            match byte {
                b' ' | b'\t' | b'\r' => self.pos += 1,
                b'\n' => {
                    self.pos += 1;
                    return Ok(Some(self.token(TokenKind::Eol, start)));
                }
                b';' => {
                    while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
                        self.pos += 1;
                    }
                }
                b'%' | b'@' => {
                    self.pos += 1;
                    let name = self.read_sigil_name(start)?;
                    let kind = if byte == b'%' {
                        TokenKind::Local(name)
                    } else {
                        TokenKind::Global(name)
                    };
                    return Ok(Some(self.token(kind, start)));
                }
                b'!' => {
                    self.pos += 1;
                    if self.peek_byte() == Some(b'{') {
                        return Ok(Some(self.token(TokenKind::Punct('!'), start)));
                    }
                    let name = self.read_sigil_name(start)?;
                    return Ok(Some(self.token(TokenKind::Meta(name), start)));
                }
                b'#' => {
                    self.pos += 1;
                    let digits = self.read_while(|b| b.is_ascii_digit());
                    let value = digits.parse::<u32>().map_err(|_| {
                        Error::parse_at(
                            "invalid attribute group number",
                            Span::new(start, self.pos),
                        )
                    })?;
                    return Ok(Some(self.token(TokenKind::AttrGroup(value), start)));
                }
                b'"' => {
                    let bytes = self.read_quoted(start)?;
                    return Ok(Some(self.token(TokenKind::Str(bytes), start)));
                }
                b'c' if self.peek_byte_at(1) == Some(b'"') => {
                    self.pos += 1;
                    let bytes = self.read_quoted(start)?;
                    return Ok(Some(self.token(TokenKind::CStr(bytes), start)));
                }
                b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'<' | b'>' | b',' | b'=' | b':'
                | b'*' => {
                    self.pos += 1;
                    return Ok(Some(self.token(TokenKind::Punct(byte as char), start)));
                }
                b'.' if self.bytes[self.pos..].starts_with(b"...") => {
                    self.pos += 3;
                    return Ok(Some(self.token(TokenKind::Ellipsis, start)));
                }
                b'-' if self.peek_byte_at(1).is_some_and(|b| b.is_ascii_digit()) => {
                    return Ok(Some(self.read_number(start)?));
                }
                b'0'..=b'9' => return Ok(Some(self.read_number(start)?)),
                b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' | b'.' | b'-' => {
                    let ident = Cow::Borrowed(self.read_while(is_ident_char));
                    return Ok(Some(self.token(TokenKind::Ident(ident), start)));
                }
                _ => {
                    return Err(Error::parse_at(
                        format!("unexpected character `{}`", byte as char),
                        Span::new(start, start + 1),
                    ));
                }
            }
        }
        Ok(None)
    }

    fn token(&self, kind: TokenKind<'a>, start: usize) -> Token<'a> {
        let quoted = matches!(kind, TokenKind::Global(_) | TokenKind::Local(_))
            && self.bytes.get(start + 1) == Some(&b'"');
        Token {
            kind,
            span: Span::new(start, self.pos),
            quoted,
        }
    }

    fn eof_token(&self) -> Token<'a> {
        Token {
            kind: TokenKind::Eof,
            span: Span::point(self.bytes.len()),
            quoted: false,
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_byte_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn read_while(&mut self, predicate: impl Fn(u8) -> bool) -> &'a str {
        let start = self.pos;
        while self.pos < self.bytes.len() && predicate(self.bytes[self.pos]) {
            self.pos += 1;
        }
        &self.source[start..self.pos]
    }

    fn read_sigil_name(&mut self, start: usize) -> Result<Cow<'a, str>> {
        if self.peek_byte() == Some(b'"') {
            let bytes = self.read_quoted(start)?;
            return String::from_utf8(bytes).map(Cow::Owned).map_err(|_| {
                Error::parse_at("name is not valid UTF-8", Span::new(start, self.pos))
            });
        }
        let name = self.read_while(is_name_char);
        if name.is_empty() {
            return Err(Error::parse_at(
                "expected a name after sigil",
                Span::new(start, self.pos),
            ));
        }
        Ok(Cow::Borrowed(name))
    }

    fn read_quoted(&mut self, start: usize) -> Result<Vec<u8>> {
        debug_assert_eq!(self.bytes[self.pos], b'"');
        self.pos += 1;
        let mut out = Vec::new();
        loop {
            if self.pos >= self.bytes.len() {
                return Err(Error::parse_at(
                    "unterminated string",
                    Span::new(start, self.pos),
                ));
            }
            let byte = self.bytes[self.pos];
            self.pos += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    if self.pos + 1 >= self.bytes.len() {
                        return Err(Error::parse_at(
                            "truncated hex escape",
                            Span::new(start, self.pos),
                        ));
                    }
                    let high = hex_value(self.bytes[self.pos]).ok_or_else(|| {
                        Error::parse_at("invalid hex escape", Span::new(self.pos, self.pos + 1))
                    })?;
                    let low = hex_value(self.bytes[self.pos + 1]).ok_or_else(|| {
                        Error::parse_at("invalid hex escape", Span::new(self.pos + 1, self.pos + 2))
                    })?;
                    self.pos += 2;
                    out.push((high << 4) | low);
                }
                other => out.push(other),
            }
        }
    }

    fn read_number(&mut self, start: usize) -> Result<Token<'a>> {
        let text = self.read_while(|byte| {
            !matches!(
                byte,
                b' ' | b'\t'
                    | b'\r'
                    | b'\n'
                    | b','
                    | b'('
                    | b')'
                    | b'['
                    | b']'
                    | b'{'
                    | b'}'
                    | b'<'
                    | b'>'
                    | b'='
                    | b':'
                    | b';'
                    | b'!'
            )
        });
        let kind = if is_float_literal(text) {
            TokenKind::Float(Cow::Borrowed(text))
        } else {
            let value = parse_int_literal(text).ok_or_else(|| {
                Error::parse_at(
                    format!("invalid integer literal `{text}`"),
                    Span::new(start, self.pos),
                )
            })?;
            TokenKind::Int(value)
        };
        Ok(self.token(kind, start))
    }
}

fn is_ident_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'$' | b'-')
}

fn is_name_char(byte: u8) -> bool {
    is_ident_char(byte)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn is_float_literal(text: &str) -> bool {
    if text.starts_with("u0x") || text.starts_with("s0x") {
        return false;
    }
    if text.starts_with("0x") {
        return true;
    }
    text.contains('.') || text.contains('e') || text.contains('E')
}

fn parse_int_literal(text: &str) -> Option<u128> {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let value = if let Some(hex) = rest
        .strip_prefix("u0x")
        .or_else(|| rest.strip_prefix("s0x"))
        .or_else(|| rest.strip_prefix("U0x"))
        .or_else(|| rest.strip_prefix("S0x"))
    {
        u128::from_str_radix(hex, 16).ok()?
    } else {
        rest.parse::<u128>().ok()?
    };
    Some(if negative {
        (!value).wrapping_add(1)
    } else {
        value
    })
}

fn parse_float_bits(text: &str, kind: FloatKind) -> Option<u64> {
    if let Some(rest) = text.strip_prefix("0x") {
        let (tag, digits) = match rest.as_bytes().first() {
            Some(b'H') | Some(b'R') | Some(b'K') | Some(b'L') | Some(b'M') => {
                (rest.as_bytes()[0] as char, &rest[1..])
            }
            _ => ('\0', rest),
        };
        let bits = u64::from_str_radix(digits, 16).ok()?;
        return Some(match tag {
            'H' => bits & 0xffff,
            'R' => bits & 0xffff,
            'K' => bits,
            'L' => bits,
            'M' => bits,
            _ => float_bits_from_f64(f64::from_bits(bits), kind),
        });
    }
    let value = text.parse::<f64>().ok()?;
    Some(float_bits_from_f64(value, kind))
}

fn float_bits_from_f64(value: f64, kind: FloatKind) -> u64 {
    let bits = value.to_bits();
    match kind {
        FloatKind::Half => f64_to_half_bits(bits) as u64,
        FloatKind::BFloat => f64_to_bfloat_bits(bits) as u64,
        FloatKind::Float => f64_to_f32_bits(bits) as u64,
        FloatKind::Double => bits,
        _ => bits,
    }
}

fn round_shift(value: u64, shift: u32) -> u64 {
    if shift == 0 {
        return value;
    }
    if shift >= 64 {
        return 0;
    }
    let truncated = value >> shift;
    let mask = (1u64 << shift) - 1;
    let remainder = value & mask;
    let halfway = 1u64 << (shift - 1);
    if remainder > halfway || (remainder == halfway && truncated & 1 == 1) {
        truncated + 1
    } else {
        truncated
    }
}

fn f64_to_f32_bits(bits: u64) -> u32 {
    let sign = ((bits >> 63) as u32) << 31;
    let exp = ((bits >> 52) & 0x7ff) as u32;
    let frac = bits & 0x000f_ffff_ffff_ffff;
    if exp == 0x7ff {
        if frac == 0 {
            return sign | 0x7f80_0000;
        }
        let payload = ((frac >> 29) as u32) & 0x007f_ffff;
        return sign | 0x7f80_0000 | payload.max(1);
    }
    (f64::from_bits(bits) as f32).to_bits()
}

fn f64_to_bfloat_bits(bits: u64) -> u16 {
    let sign = ((bits >> 48) & 0x8000) as u16;
    let exp = ((bits >> 52) & 0x7ff) as u32;
    let frac = bits & 0x000f_ffff_ffff_ffff;
    if exp == 0x7ff {
        if frac == 0 {
            return sign | 0x7f80;
        }
        let payload = ((frac >> 45) as u16) & 0x007f;
        return sign | 0x7f80 | payload.max(1);
    }
    if exp == 0 {
        return sign;
    }
    let unbiased = exp as i32 - 1023;
    if unbiased > 127 {
        return sign | 0x7f80;
    }
    let significand = 0x0010_0000_0000_0000u64 | frac;
    if unbiased >= -126 {
        let mut rounded = round_shift(significand, 45) as u32;
        let mut exponent = unbiased + 127;
        if rounded >= 0x100 {
            rounded = 0;
            exponent += 1;
        }
        if exponent >= 0xff {
            return sign | 0x7f80;
        }
        return sign | ((exponent as u16) << 7) | (rounded as u16 & 0x007f);
    }
    if unbiased < -134 {
        return sign;
    }
    let shift = (-unbiased - 81) as u32;
    let rounded = round_shift(significand, shift);
    if rounded >= 0x80 {
        sign | 0x0080
    } else {
        sign | rounded as u16
    }
}

fn f64_to_half_bits(bits: u64) -> u16 {
    let sign = ((bits >> 48) & 0x8000) as u16;
    let exp = ((bits >> 52) & 0x7ff) as u32;
    let frac = bits & 0x000f_ffff_ffff_ffff;
    if exp == 0x7ff {
        if frac == 0 {
            return sign | 0x7c00;
        }
        let payload = ((frac >> 42) as u16) & 0x03ff;
        return sign | 0x7c00 | payload.max(1);
    }
    if exp == 0 {
        return sign;
    }
    let unbiased = exp as i32 - 1023;
    if unbiased > 15 {
        return sign | 0x7c00;
    }
    let significand = 0x0010_0000_0000_0000u64 | frac;
    if unbiased >= -14 {
        let mut rounded = round_shift(significand, 42) as u32;
        let mut exponent = unbiased + 15;
        if rounded >= 0x800 {
            rounded = 0;
            exponent += 1;
        }
        if exponent >= 0x1f {
            return sign | 0x7c00;
        }
        return sign | ((exponent as u16) << 10) | (rounded as u16 & 0x03ff);
    }
    if unbiased < -25 {
        return sign;
    }
    let shift = (28 - unbiased) as u32;
    let rounded = round_shift(significand, shift);
    if rounded >= 0x400 {
        sign | 0x0400
    } else {
        sign | rounded as u16
    }
}

fn is_unnamed_symbol(name: &str, quoted: bool) -> bool {
    name.is_empty()
        || (!quoted && !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit()))
}

fn is_type_attribute(name: &str) -> bool {
    matches!(
        name,
        "byval" | "sret" | "inalloca" | "preallocated" | "byref" | "elementtype"
    )
}

fn normalize_group_attribute(attr: Attribute) -> Attribute {
    let text = attr.0;
    if let Some(value) = text
        .strip_prefix("alignstack(")
        .and_then(|value| value.strip_suffix(')'))
    {
        return Attribute(format!("alignstack={value}"));
    }
    if let Some(value) = text.strip_prefix("align ") {
        return Attribute(format!("align={value}"));
    }
    if let Some(value) = text
        .strip_prefix("align(")
        .and_then(|value| value.strip_suffix(')'))
    {
        return Attribute(format!("align={value}"));
    }
    Attribute(text)
}

fn token_text(kind: &TokenKind<'_>) -> String {
    match kind {
        TokenKind::Local(name) => format!("%{name}"),
        TokenKind::Global(name) => format!("@{name}"),
        TokenKind::Meta(name) => format!("!{name}"),
        TokenKind::AttrGroup(value) => format!("#{value}"),
        TokenKind::Ident(text) => text.to_string(),
        TokenKind::Int(value) => value.to_string(),
        TokenKind::Float(text) => text.to_string(),
        TokenKind::Str(bytes) => quote_bytes(bytes),
        TokenKind::CStr(bytes) => format!("c{}", quote_bytes(bytes)),
        TokenKind::Punct(ch) => ch.to_string(),
        TokenKind::Ellipsis => "...".to_string(),
        TokenKind::Eol => "\n".to_string(),
        TokenKind::Eof => String::new(),
    }
}

fn is_attr_keyword(text: &str) -> bool {
    matches!(
        text,
        "alwaysinline"
            | "builtin"
            | "cold"
            | "convergent"
            | "disable_sanitizer_instrumentation"
            | "hot"
            | "inaccessiblememonly"
            | "inaccessiblemem_or_argmemonly"
            | "inlinehint"
            | "jumptable"
            | "memory"
            | "minsize"
            | "mustprogress"
            | "naked"
            | "nobuiltin"
            | "nocallback"
            | "nocf_check"
            | "nodivergences"
            | "noduplicate"
            | "noinline"
            | "nomerge"
            | "nonlazybind"
            | "noprofile"
            | "norecurse"
            | "noreturn"
            | "nosync"
            | "nounwind"
            | "null_pointer_is_valid"
            | "optdebug"
            | "optforfuzzing"
            | "optnone"
            | "optsize"
            | "presplitcoroutine"
            | "readnone"
            | "readonly"
            | "returns_twice"
            | "safestack"
            | "speculatable"
            | "speculative_load_hardening"
            | "ssp"
            | "sspreq"
            | "sspstrong"
            | "strictfp"
            | "uwtable"
            | "willreturn"
            | "nofree"
            | "nosanitize_coverage"
            | "noundef"
            | "zeroext"
            | "signext"
            | "inreg"
            | "noalias"
            | "nonnull"
            | "align"
            | "alignstack"
            | "dereferenceable"
            | "dereferenceable_or_null"
            | "byval"
            | "byref"
            | "sret"
            | "elementtype"
            | "swiftself"
            | "swifterror"
            | "immarg"
            | "returned"
            | "nofpclass"
            | "range"
            | "captures"
            | "allocalign"
            | "allocptr"
            | "nest"
            | "nocapture"
            | "writeonly"
            | "dead_on_unwind"
            | "writable"
            | "initializes"
            | "preallocated"
            | "inalloca"
            | "sanitize_address"
            | "sanitize_memory"
            | "sanitize_thread"
            | "sanitize_hwaddress"
            | "sanitize_memtag"
            | "shadowcallstack"
            | "preserve_allcc"
            | "preserve_mostcc"
            | "swiftcc"
            | "tailcc"
            | "fastcc"
            | "coldcc"
    )
}

struct ParsedCall {
    fastmath: FastMath,
    attrs: Vec<Attribute>,
    ret_attrs: Vec<Attribute>,
    ret_ty: TypeId,
    fn_ty: TypeId,
    callee: Operand,
    args: Vec<Operand>,
    arg_attrs: Vec<Arc<Vec<Attribute>>>,
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    tokens: VecDeque<Token<'a>>,
    lex_error: Option<Error>,
    finished: bool,
    module: Module,
    local_names: FastHashMap<Symbol, LocalId>,
    next_value: u32,
    next_unnamed: u64,
    unnamed_symbols: FastHashMap<u32, Symbol>,
}

impl<'a> Parser<'a> {
    fn new(lexer: Lexer<'a>) -> Self {
        Self {
            lexer,
            tokens: VecDeque::with_capacity(2),
            lex_error: None,
            finished: false,
            module: Module::new(),
            local_names: FastHashMap::with_capacity_and_hasher(64, Default::default()),
            next_value: 0,
            next_unnamed: 0,
            unnamed_symbols: FastHashMap::default(),
        }
    }

    fn ensure(&mut self, lookahead: usize) {
        while self.tokens.len() < lookahead && !self.finished {
            match self.lexer.next_token() {
                Ok(Some(token)) => self.tokens.push_back(token),
                Ok(None) => {
                    self.tokens.push_back(self.lexer.eof_token());
                    self.finished = true;
                }
                Err(error) => {
                    self.lex_error = Some(error);
                    self.tokens.push_back(self.lexer.eof_token());
                    self.finished = true;
                }
            }
        }
    }

    fn parse_top_level(&mut self) -> Result<()> {
        loop {
            self.skip_eol();
            if self.at_eof() {
                return Ok(());
            }
            match self.peek_kind().clone() {
                TokenKind::Ident(text) => match &*text {
                    "source_filename" => {
                        self.advance();
                        self.expect_punct('=')?;
                        self.module.source_filename = Some(self.expect_string()?);
                        self.expect_eol()?;
                    }
                    "target" => {
                        self.advance();
                        let which = self.expect_ident()?;
                        self.expect_punct('=')?;
                        let value = self.expect_string()?;
                        match which.as_str() {
                            "datalayout" => self.module.data_layout = Some(value),
                            "triple" => self.module.target_triple = Some(value),
                            other => {
                                return Err(
                                    self.error(format!("unknown target property `{other}`"))
                                );
                            }
                        }
                        self.expect_eol()?;
                    }
                    "attributes" => {
                        self.advance();
                        self.parse_attribute_group()?;
                    }
                    "declare" => self.parse_function(true)?,
                    "define" => self.parse_function(false)?,
                    "uselistorder" | "uselistorder_bb" => self.skip_line(),
                    other => {
                        return Err(self.error(format!("unexpected top-level keyword `{other}`")));
                    }
                },
                TokenKind::Local(_) => self.parse_type_definition()?,
                TokenKind::Global(_) => self.parse_global()?,
                TokenKind::Meta(_) | TokenKind::Punct('!') => self.skip_line(),
                TokenKind::Eol => self.advance(),
                other => {
                    return Err(self.error(format!("unexpected token {other:?} at top level")));
                }
            }
        }
    }

    fn parse_type_definition(&mut self) -> Result<()> {
        let (name, _) = self.expect_local()?;
        self.expect_punct('=')?;
        self.expect_ident_is("type")?;
        let ty = self.parse_type()?;
        match self.module.types.get(ty).clone() {
            Type::Struct {
                fields,
                packed,
                name: None,
            } => {
                let id = self
                    .module
                    .types
                    .define_named(&name, fields, packed)
                    .map_err(|message| self.error(message))?;
                if !self.module.named_types.contains(&id) {
                    self.module.named_types.push(id);
                }
            }
            Type::Opaque => {
                let id = self.module.types.define_named_opaque(&name);
                if !self.module.named_types.contains(&id) {
                    self.module.named_types.push(id);
                }
            }
            Type::Struct { name: Some(_), .. } => {
                return Err(self.error("a named type cannot be defined as another named type"));
            }
            _ => return Err(self.error("a named type must be a struct")),
        }
        self.expect_eol()
    }

    fn parse_attribute_group(&mut self) -> Result<()> {
        let group = match self.peek_kind() {
            TokenKind::AttrGroup(value) => *value,
            _ => return Err(self.error("expected an attribute group number")),
        };
        self.advance();
        self.expect_punct('=')?;
        self.expect_punct('{')?;
        let mut attrs = Vec::new();
        loop {
            self.skip_eol();
            if self.eat_punct('}') {
                break;
            }
            if self.at_eof() {
                return Err(self.error("unterminated attribute group"));
            }
            let attr = match self.peek_kind() {
                TokenKind::Str(_) | TokenKind::AttrGroup(_) => self.parse_attribute_token()?,
                _ => self.parse_attribute()?,
            };
            attrs.push(normalize_group_attribute(attr));
        }
        let text = attrs
            .into_iter()
            .map(|attr| attr.0)
            .collect::<Vec<_>>()
            .join(" ");
        self.module.attr_groups.push((group, text));
        self.expect_eol()
    }

    fn parse_global(&mut self) -> Result<()> {
        let (name, span, quoted) = self.expect_global()?;
        let symbol = self.definition_symbol(&name, quoted, "global", span)?;
        let unnamed = is_unnamed_symbol(&name, quoted);
        self.expect_punct('=')?;
        let mut linkage = Linkage::External;
        let mut visibility = Visibility::Default;
        let mut dso_local = false;
        let mut tls_model = ThreadLocal::None;
        let mut unnamed_addr = false;
        let mut local_unnamed_addr = false;
        let mut is_const = false;
        let mut saw_kind = false;
        loop {
            let text = match self.peek_kind() {
                TokenKind::Ident(text) => text.clone(),
                _ => break,
            };
            if let Some(parsed) = Linkage::parse(&text) {
                linkage = parsed;
                self.advance();
                continue;
            }
            if let Some(parsed) = Visibility::parse(&text) {
                visibility = parsed;
                self.advance();
                continue;
            }
            match &*text {
                "dso_local" | "dso_preemptable" => {
                    dso_local = &*text == "dso_local";
                    self.advance();
                }
                "thread_local" => {
                    self.advance();
                    tls_model = ThreadLocal::GeneralDynamic;
                    if self.eat_punct('(') {
                        let model = self.expect_ident()?;
                        tls_model = match model.as_str() {
                            "generaldynamic" => ThreadLocal::GeneralDynamic,
                            "localdynamic" => ThreadLocal::LocalDynamic,
                            "initialexec" => ThreadLocal::InitialExec,
                            "localexec" => ThreadLocal::LocalExec,
                            other => {
                                return Err(self.error(format!("unsupported TLS model `{other}`")));
                            }
                        };
                        self.expect_punct(')')?;
                    }
                }
                "unnamed_addr" => {
                    unnamed_addr = true;
                    self.advance();
                }
                "local_unnamed_addr" => {
                    local_unnamed_addr = true;
                    self.advance();
                }
                "externally_initialized" => {
                    self.advance();
                }
                "addrspace" => {
                    self.advance();
                    self.expect_punct('(')?;
                    let address_space = self.expect_int()?;
                    self.expect_punct(')')?;
                    if address_space != 0 {
                        return Err(self.error("nonzero global address spaces are not supported"));
                    }
                }
                "global" => {
                    is_const = false;
                    saw_kind = true;
                    self.advance();
                    break;
                }
                "constant" => {
                    is_const = true;
                    saw_kind = true;
                    self.advance();
                    break;
                }
                _ => break,
            }
        }
        if !saw_kind {
            return Err(self.error("expected `global` or `constant` in global definition"));
        }
        let ty = self.parse_type()?;
        let init = if self.at_eol() || self.at_punct(',') || self.at_eof() {
            None
        } else {
            Some(self.parse_constant(ty)?)
        };
        let mut align = None;
        let mut section = None;
        while self.eat_punct(',') {
            if self.at_eol() || self.at_eof() {
                break;
            }
            let key = self.expect_ident()?;
            match key.as_str() {
                "align" => align = Some(self.expect_int()? as u64),
                "section" => section = Some(self.expect_string()?),
                "comdat" => {
                    if self.eat_punct('(') {
                        let _ = self.expect_global()?;
                        self.expect_punct(')')?;
                    }
                }
                "!dbg" | "!annotation" | "!associated" => {
                    let _ = self.take();
                }
                other => {
                    return Err(self.error(format!("unsupported global property `{other}`")));
                }
            }
        }
        self.expect_eol()?;
        let global = Global {
            name: symbol,
            ty,
            init,
            linkage,
            visibility,
            is_const,
            tls_model,
            unnamed_addr,
            local_unnamed_addr,
            dso_local,
            align,
            section,
        };
        let id = self.module.global_or_insert(symbol);
        self.module.define_global(id, global);
        if unnamed {
            self.module.mark_unnamed_global(id);
        }
        Ok(())
    }

    fn parse_function(&mut self, is_declaration: bool) -> Result<()> {
        self.advance();
        self.local_names.clear();
        self.next_value = 0;
        let mut linkage = Linkage::External;
        let mut visibility = Visibility::Default;
        let mut dso_local = false;
        let mut unnamed_addr = false;
        let mut local_unnamed_addr = false;
        let mut attrs = Vec::new();
        loop {
            let text = match self.peek_kind() {
                TokenKind::Ident(text) => text.clone(),
                TokenKind::AttrGroup(_) | TokenKind::Str(_) => {
                    attrs.push(self.parse_attribute_token()?);
                    continue;
                }
                _ => break,
            };
            if let Some(parsed) = Linkage::parse(&text) {
                linkage = parsed;
                self.advance();
                continue;
            }
            if let Some(parsed) = Visibility::parse(&text) {
                visibility = parsed;
                self.advance();
                continue;
            }
            match &*text {
                "dso_local" | "dso_preemptable" => {
                    dso_local = &*text == "dso_local";
                    self.advance();
                }
                "unnamed_addr" => {
                    unnamed_addr = true;
                    self.advance();
                }
                "local_unnamed_addr" => {
                    local_unnamed_addr = true;
                    self.advance();
                }
                other if is_attr_keyword(other) => {
                    attrs.push(self.parse_attribute()?);
                }
                _ => break,
            }
        }
        let ret_ty = self.parse_type()?;
        let (name, span, quoted) = self.expect_global()?;
        let symbol = self.definition_symbol(&name, quoted, "function", span)?;
        let unnamed = is_unnamed_symbol(&name, quoted);
        let mut function = Function::new(symbol, 0);
        function.linkage = linkage;
        function.visibility = visibility;
        function.dso_local = dso_local;
        function.unnamed_addr = unnamed_addr;
        function.local_unnamed_addr = local_unnamed_addr;
        function.is_declaration = is_declaration;
        function.ret_attrs = attrs.into();
        self.expect_punct('(')?;
        let mut param_types = Vec::new();
        let mut varargs = false;
        if !self.at_punct(')') {
            loop {
                if self.eat_ellipsis() {
                    varargs = true;
                    break;
                }
                let ty = self.parse_type()?;
                let param_attrs = self.parse_param_attrs()?;
                let param_name = match self.peek_kind() {
                    TokenKind::Local(name) => {
                        let name = name.clone();
                        self.advance();
                        Some(name)
                    }
                    _ => None,
                };
                let local_symbol = if is_declaration {
                    param_name.map(|name| self.module.intern(&name))
                } else {
                    Some(self.local_symbol(param_name)?)
                };
                let local_id = function.add_param(local_symbol, ty, param_attrs);
                if let Some(symbol) = local_symbol {
                    self.local_names.insert(symbol, local_id);
                }
                param_types.push(ty);
                if !self.eat_punct(',') {
                    break;
                }
            }
        }
        self.expect_punct(')')?;
        function.ty = self.module.types.function(ret_ty, param_types, varargs);
        let mut function_attrs = Vec::new();
        self.parse_trailing_attrs(&mut function, &mut function_attrs, !is_declaration)?;
        if let Some(index) = function_attrs
            .iter()
            .position(|attr| attr.0 == "local_unnamed_addr")
        {
            function_attrs.remove(index);
            function.local_unnamed_addr = true;
        }
        if let Some(index) = function_attrs
            .iter()
            .position(|attr| attr.0 == "unnamed_addr")
        {
            function_attrs.remove(index);
            function.unnamed_addr = true;
        }
        let mut align = None;
        function_attrs.retain(|attr| {
            if let Some(value) = attr.0.strip_prefix("align ") {
                align = value.trim().parse().ok();
                false
            } else {
                true
            }
        });
        function.align = align;
        function.attrs = function_attrs.into();
        if is_declaration {
            if self.at_eol() {
                self.advance();
            }
        } else {
            self.expect_punct('{')?;
            for (index, param) in function.params.iter().enumerate() {
                if let Some(name) = param.name {
                    self.local_names.insert(name, index as LocalId);
                }
            }
            self.parse_body(&mut function)?;
        }
        let id = self.module.function_or_insert(symbol);
        if !self.module.functions[id].is_declaration
            || self.module.types.is_valid(self.module.functions[id].ty)
        {
            return Err(self.error(format!("redefinition of function @{name}")));
        }
        self.module.define_function(id, function);
        if unnamed {
            self.module.mark_unnamed_function(id);
        }
        Ok(())
    }

    fn parse_param_attrs(&mut self) -> Result<Vec<Attribute>> {
        let mut attrs = Vec::new();
        loop {
            match self.peek_kind() {
                TokenKind::Ident(text) if is_attr_keyword(text) => {
                    attrs.push(self.parse_attribute()?);
                }
                TokenKind::AttrGroup(_) | TokenKind::Str(_) => {
                    attrs.push(self.parse_attribute_token()?);
                }
                _ => return Ok(attrs),
            }
        }
    }

    fn parse_trailing_attrs(
        &mut self,
        function: &mut Function,
        attrs: &mut Vec<Attribute>,
        stop_at_brace: bool,
    ) -> Result<()> {
        loop {
            if stop_at_brace && self.at_punct('{') {
                return Ok(());
            }
            if self.at_eof() {
                return Ok(());
            }
            if self.at_eol() {
                self.advance();
                if !stop_at_brace {
                    return Ok(());
                }
                continue;
            }
            match self.peek_kind().clone() {
                TokenKind::AttrGroup(value) => {
                    self.advance();
                    attrs.push(Attribute(format!("#{value}")));
                }
                TokenKind::Str(_) => attrs.push(self.parse_attribute_token()?),
                TokenKind::Ident(text) => {
                    if is_attr_keyword(&text) {
                        attrs.push(self.parse_attribute()?);
                    } else {
                        match &*text {
                            "personality" => {
                                self.advance();
                                let ty = self.parse_type()?;
                                function.personality = Some(self.parse_value(function, ty)?);
                            }
                            "section" | "gc" | "prefix" | "prologue" => {
                                let value = self.expect_string()?;
                                attrs.push(Attribute(format!(
                                    "{text} {}",
                                    quote_bytes(value.as_bytes())
                                )));
                            }
                            "align" => {
                                let value = self.expect_int()?;
                                attrs.push(Attribute(format!("align {value}")));
                            }
                            "unnamed_addr" | "local_unnamed_addr" => {
                                attrs.push(Attribute(text.to_string()));
                                self.advance();
                            }
                            "comdat" => {
                                let mut text = String::from("comdat");
                                if self.eat_punct('(') {
                                    let (name, _, _) = self.expect_global()?;
                                    self.expect_punct(')')?;
                                    text.push_str(&format!("(@{name})"));
                                }
                                attrs.push(Attribute(text));
                            }
                            other => {
                                return Err(
                                    self.error(format!("unsupported function attribute `{other}`"))
                                );
                            }
                        }
                    }
                }
                TokenKind::Punct('!') | TokenKind::Meta(_) => self.skip_line(),
                other => {
                    return Err(self.error(format!("unexpected token {other:?} in attributes")));
                }
            }
        }
    }

    fn parse_attribute_token(&mut self) -> Result<Attribute> {
        match self.peek_kind().clone() {
            TokenKind::AttrGroup(value) => {
                self.advance();
                Ok(Attribute(format!("#{value}")))
            }
            TokenKind::Str(key) => {
                self.advance();
                let key = quote_bytes(&key);
                if self.eat_punct('=') {
                    let value = match self.peek_kind().clone() {
                        TokenKind::Str(bytes) => {
                            self.advance();
                            bytes
                        }
                        other => {
                            return Err(self.error(format!("expected a string, found {other:?}")));
                        }
                    };
                    Ok(Attribute(format!("{key}={}", quote_bytes(&value))))
                } else {
                    Ok(Attribute(key))
                }
            }
            _ => Err(self.error("expected an attribute")),
        }
    }

    fn parse_attribute(&mut self) -> Result<Attribute> {
        let name = self.expect_ident()?;
        let mut text = name.clone();
        if is_type_attribute(&name) && self.at_punct('(') {
            self.expect_punct('(')?;
            let ty = self.parse_type()?;
            self.expect_punct(')')?;
            return Ok(Attribute(format!(
                "{name}({})",
                self.module.types.display(ty)
            )));
        }
        if self.at_punct('(') {
            self.consume_balanced_text(&mut text)?;
        } else if self.at_punct('=') {
            self.advance();
            let value = self.expect_int()?;
            text = if name == "align" {
                format!("align {value}")
            } else {
                format!("{name}({value})")
            };
        } else if name == "align" {
            let value = self.expect_int()?;
            text.push_str(&format!(" {value}"));
        }
        Ok(Attribute(text))
    }

    fn consume_balanced_text(&mut self, out: &mut String) -> Result<()> {
        self.expect_punct('(')?;
        out.push('(');
        let mut depth = 1usize;
        let mut first = true;
        while depth > 0 {
            if self.at_eof() {
                return Err(self.error("unterminated attribute argument list"));
            }
            let token = self.take();
            match token.kind {
                TokenKind::Punct('(') => {
                    depth += 1;
                    out.push('(');
                }
                TokenKind::Punct(')') => {
                    depth -= 1;
                    if depth > 0 {
                        out.push(')');
                    }
                }
                TokenKind::Punct(',') => out.push_str(", "),
                TokenKind::Punct(':') => out.push_str(": "),
                other => {
                    if !first && matches!(other, TokenKind::Ident(_)) {
                        out.push(' ');
                    }
                    out.push_str(&token_text(&other));
                }
            }
            first = false;
        }
        out.push(')');
        Ok(())
    }

    fn local_symbol(&mut self, name: Option<Cow<'a, str>>) -> Result<Symbol> {
        match name {
            Some(name) => {
                if let Ok(value) = name.parse::<u32>() {
                    self.next_value = self.next_value.max(value.saturating_add(1));
                }
                Ok(self.module.intern(&name))
            }
            None => {
                let value = self.next_value;
                self.next_value += 1;
                Ok(self.module.intern(&value.to_string()))
            }
        }
    }

    fn parse_body(&mut self, function: &mut Function) -> Result<()> {
        let mut current: Option<BlockId> = None;
        loop {
            self.skip_eol();
            if self.eat_punct('}') {
                return Ok(());
            }
            if self.at_eof() {
                return Err(self.error("unterminated function body"));
            }
            if self.is_label_start() {
                let name = self.parse_label_name()?;
                let block = function.block_or_insert(name);
                function.mark_block_defined(block);
                current = Some(block);
                continue;
            }
            let block = match current {
                Some(block) => block,
                None => {
                    let name = self.auto_block_name();
                    let block = function.block_or_insert(name);
                    function.mark_block_defined(block);
                    current = Some(block);
                    block
                }
            };
            let result_name = if let TokenKind::Local(name) = self.peek_kind() {
                if matches!(self.peek_kind_at(1), TokenKind::Punct('=')) {
                    let name = name.clone();
                    self.advance();
                    self.advance();
                    Some(name)
                } else {
                    None
                }
            } else {
                None
            };
            let (kind, result_ty) = self.parse_instruction(function)?;
            let inst_id = function.add_inst(block, Instruction { result: None, kind });
            if result_name.is_some() && result_ty.is_none() {
                return Err(self.error("instruction does not produce a value"));
            }
            if let Some(ty) = result_ty {
                let symbol = self.local_symbol(result_name)?;
                let local = match self.local_names.get(&symbol).copied() {
                    Some(existing)
                        if !function.local(existing).is_param
                            && function.local(existing).def.is_none() =>
                    {
                        let previous = function.local(existing).ty;
                        if previous != INVALID_TYPE && previous != ty {
                            return Err(self.error(format!(
                                "forward reference type mismatch for %{}",
                                self.module.name(symbol)
                            )));
                        }
                        function.define_local(existing, ty, inst_id);
                        existing
                    }
                    Some(_) => {
                        return Err(self.error(format!(
                            "redefinition of local value %{}",
                            self.module.name(symbol)
                        )));
                    }
                    None => {
                        let local = function.add_local(Some(symbol), ty, inst_id);
                        self.local_names.insert(symbol, local);
                        local
                    }
                };
                function.insts[inst_id].result = Some(local);
            }
            if self.at_eol() {
                self.advance();
            }
        }
    }

    fn parse_label_name(&mut self) -> Result<Symbol> {
        let token = self.take();
        let name = match token.kind {
            TokenKind::Ident(name) | TokenKind::Local(name) => name,
            TokenKind::Int(value) => Cow::Owned(value.to_string()),
            TokenKind::Str(bytes) => Cow::Owned(
                String::from_utf8(bytes)
                    .map_err(|_| Error::parse_at("label is not valid UTF-8", token.span))?,
            ),
            _ => return Err(Error::parse_at("expected a basic block label", token.span)),
        };
        self.expect_punct(':')?;
        if let Ok(value) = name.parse::<u32>() {
            self.next_value = self.next_value.max(value.saturating_add(1));
        }
        Ok(self.module.intern(&name))
    }

    fn auto_block_name(&mut self) -> Symbol {
        let value = self.next_value;
        self.next_value += 1;
        self.module.intern(&value.to_string())
    }

    fn is_label_start(&self) -> bool {
        match self.peek_kind() {
            TokenKind::Ident(_) | TokenKind::Int(_) | TokenKind::Str(_) => {
                matches!(self.peek_kind_at(1), TokenKind::Punct(':'))
            }
            _ => false,
        }
    }

    fn parse_type(&mut self) -> Result<TypeId> {
        let token = self.take();
        let span = token.span;
        let base = match token.kind {
            TokenKind::Ident(text) => match &*text {
                "void" => self.module.types.void(),
                "label" => self.module.types.intern(Type::Label),
                "token" => self.module.types.intern(Type::Token),
                "metadata" => self.module.types.intern(Type::Metadata),
                "opaque" => self.module.types.intern(Type::Opaque),
                "ptr" => {
                    if self.eat_ident("addrspace") {
                        self.expect_punct('(')?;
                        let address_space = self.expect_int()?;
                        self.expect_punct(')')?;
                        if address_space != 0 {
                            return Err(
                                self.error("nonzero pointer address spaces are not supported")
                            );
                        }
                    }
                    self.module.types.pointer()
                }
                "half" => self.module.types.float(FloatKind::Half),
                "bfloat" => self.module.types.float(FloatKind::BFloat),
                "float" => self.module.types.float(FloatKind::Float),
                "double" => self.module.types.float(FloatKind::Double),
                "fp128" => self.module.types.float(FloatKind::Fp128),
                "x86_fp80" => self.module.types.float(FloatKind::X86Fp80),
                "ppc_fp128" => self.module.types.float(FloatKind::PpcFp128),
                other => {
                    if let Some(bits) = other.strip_prefix('i') {
                        if let Ok(bits) = bits.parse::<u32>() {
                            self.module.types.int(bits)
                        } else {
                            return Err(self.error(format!("invalid integer type `{other}`")));
                        }
                    } else {
                        return Err(self.error(format!("expected a type, found `{other}`")));
                    }
                }
            },
            TokenKind::Local(name) => self.module.types.named(&name),
            TokenKind::Punct('[') => {
                let len = self.expect_int()? as u64;
                self.expect_ident_is("x")?;
                let elem = self.parse_type()?;
                self.expect_punct(']')?;
                self.module.types.array(elem, len)
            }
            TokenKind::Punct('<') => {
                if self.eat_punct('{') {
                    let fields = self.parse_struct_fields('}')?;
                    self.expect_punct('>')?;
                    self.module.types.intern(Type::Struct {
                        fields,
                        packed: true,
                        name: None,
                    })
                } else {
                    let scalable = self.eat_ident("vscale");
                    if scalable {
                        self.expect_ident_is("x")?;
                    }
                    let len = self.expect_int()? as u32;
                    self.expect_ident_is("x")?;
                    let elem = self.parse_type()?;
                    self.expect_punct('>')?;
                    self.module.types.intern(Type::Vector {
                        elem,
                        len,
                        scalable,
                    })
                }
            }
            TokenKind::Punct('{') => {
                let fields = self.parse_struct_fields('}')?;
                self.module.types.intern(Type::Struct {
                    fields,
                    packed: false,
                    name: None,
                })
            }
            other => {
                return Err(Error::parse_at(
                    format!("expected a type, found {other:?}"),
                    span,
                ));
            }
        };
        if self.at_punct('(') {
            self.advance();
            let mut params = Vec::new();
            let mut varargs = false;
            if !self.at_punct(')') {
                loop {
                    if self.eat_ellipsis() {
                        varargs = true;
                        break;
                    }
                    params.push(self.parse_type()?);
                    if !self.eat_punct(',') {
                        break;
                    }
                }
            }
            self.expect_punct(')')?;
            return Ok(self.module.types.function(base, params, varargs));
        }
        Ok(base)
    }

    fn parse_struct_fields(&mut self, close: char) -> Result<Vec<TypeId>> {
        let mut fields = Vec::new();
        if self.eat_punct(close) {
            return Ok(fields);
        }
        loop {
            fields.push(self.parse_type()?);
            if !self.eat_punct(',') {
                break;
            }
        }
        self.expect_punct(close)?;
        Ok(fields)
    }

    fn parse_constant(&mut self, ty: TypeId) -> Result<ConstId> {
        let token = self.take();
        let span = token.span;
        let quoted = token.quoted;
        match token.kind {
            TokenKind::Int(value) => {
                let bits =
                    self.module.types.get(ty).int_bits().ok_or_else(|| {
                        self.error("integer constant used with a non-integer type")
                    })?;
                if bits > 128 {
                    return Err(self.error(format!(
                        "integer constants wider than 128 bits are not supported (found i{bits})"
                    )));
                }
                let value = mask_int(value, bits);
                Ok(self.module.const_int(ty, value))
            }
            TokenKind::Float(text) => {
                let kind = match self.module.types.get(ty) {
                    Type::Float(kind) => *kind,
                    _ => return Err(self.error("floating constant used with a non-float type")),
                };
                let bits = parse_float_bits(&text, kind)
                    .ok_or_else(|| self.error(format!("invalid floating literal `{text}`")))?;
                Ok(self.module.const_float_bits(ty, bits))
            }
            TokenKind::CStr(bytes) => Ok(self
                .module
                .add_constant(Constant::new(ty, ConstKind::String(bytes)))),
            TokenKind::Meta(name) => Ok(self.module.add_constant(Constant::new(
                ty,
                ConstKind::MetadataString(name.into_owned()),
            ))),
            TokenKind::Global(name) => {
                let symbol = self.reference_symbol(&name, quoted, span)?;
                let kind = if let Some(id) = self.module.function_named(symbol) {
                    ConstKind::Function(id)
                } else if let Some(id) = self.module.global_named(symbol) {
                    ConstKind::Global(id)
                } else {
                    ConstKind::Function(self.module.function_or_insert(symbol))
                };
                Ok(self.module.add_constant(Constant::new(ty, kind)))
            }
            TokenKind::Ident(text) => {
                if &*text == "getelementptr" {
                    return self.parse_const_gep(ty, span);
                }
                if &*text == "blockaddress" {
                    return self.parse_const_blockaddress(ty, span);
                }
                if let Some(op) = CastOp::parse(&text) {
                    return self.parse_const_cast(ty, op, span);
                }
                let kind = match &*text {
                    "true" => ConstKind::Int(1),
                    "false" => ConstKind::Int(0),
                    "null" => ConstKind::Null,
                    "undef" => ConstKind::Undef,
                    "poison" => ConstKind::Poison,
                    "zeroinitializer" => ConstKind::ZeroInitializer,
                    other => {
                        return Err(self.error(format!("unsupported constant `{other}`")));
                    }
                };
                Ok(self.module.add_constant(Constant::new(ty, kind)))
            }
            TokenKind::Punct('[') => {
                let elem_ty = match self.module.types.get(ty) {
                    Type::Array { elem, .. } | Type::Vector { elem, .. } => *elem,
                    _ => return Err(self.error("array constant used with a non-array type")),
                };
                let mut elems = Vec::new();
                if !self.eat_punct(']') {
                    loop {
                        let _ = self.parse_type()?;
                        elems.push(self.parse_constant(elem_ty)?);
                        if !self.eat_punct(',') {
                            break;
                        }
                    }
                    self.expect_punct(']')?;
                }
                Ok(self.module.const_aggregate(ty, elems))
            }
            TokenKind::Punct('<') => {
                if self.eat_punct('{') {
                    let id = self.parse_struct_constant(ty)?;
                    self.expect_punct('>')?;
                    Ok(id)
                } else {
                    let elem_ty = match self.module.types.get(ty) {
                        Type::Vector { elem, .. } => *elem,
                        _ => return Err(self.error("vector constant used with a non-vector type")),
                    };
                    let mut elems = Vec::new();
                    if !self.eat_punct('>') {
                        loop {
                            let _ = self.parse_type()?;
                            elems.push(self.parse_constant(elem_ty)?);
                            if !self.eat_punct(',') {
                                break;
                            }
                        }
                        self.expect_punct('>')?;
                    }
                    Ok(self.module.const_aggregate(ty, elems))
                }
            }
            TokenKind::Punct('{') => self.parse_struct_constant(ty),
            TokenKind::Local(name) => Err(Error::parse_at(
                format!("local value %{name} is not a constant"),
                span,
            )),
            other => Err(Error::parse_at(
                format!("expected a constant, found {other:?}"),
                span,
            )),
        }
    }

    fn parse_const_blockaddress(&mut self, result_ty: TypeId, span: Span) -> Result<ConstId> {
        if !matches!(self.module.types.get(result_ty), Type::Pointer) {
            return Err(Error::parse_at("blockaddress must produce a pointer", span));
        }
        self.expect_punct('(')?;
        let (name, name_span, quoted) = self.expect_global()?;
        let symbol = self.reference_symbol(&name, quoted, name_span)?;
        let function = self
            .module
            .function_named(symbol)
            .unwrap_or_else(|| self.module.function_or_insert(symbol));
        self.expect_punct(',')?;
        let block = self.expect_local()?.0;
        let block = self.module.intern(&block);
        self.expect_punct(')')?;
        Ok(self.module.add_constant(Constant::new(
            result_ty,
            ConstKind::BlockAddress { function, block },
        )))
    }

    fn parse_const_gep(&mut self, result_ty: TypeId, span: Span) -> Result<ConstId> {
        if !matches!(self.module.types.get(result_ty), Type::Pointer) {
            return Err(Error::parse_at(
                "getelementptr constant expression must produce a pointer",
                span,
            ));
        }
        let inbounds = self.eat_ident("inbounds");
        self.expect_punct('(')?;
        let source_ty = self.parse_type()?;
        self.expect_punct(',')?;
        let ptr_ty = self.parse_type()?;
        if !matches!(self.module.types.get(ptr_ty), Type::Pointer) {
            return Err(Error::parse_at(
                "getelementptr constant expression base must be a scalar pointer",
                span,
            ));
        }
        let ptr_const = self.parse_constant(ptr_ty)?;
        let ptr = Operand::constant(ptr_ty, ptr_const);
        let mut indices = Vec::new();
        while self.eat_punct(',') {
            let index_ty = self.parse_type()?;
            let index = self.parse_constant(index_ty)?;
            indices.push(GepIndex {
                ty: index_ty,
                value: Operand::constant(index_ty, index),
            });
        }
        self.expect_punct(')')?;
        if indices.is_empty() {
            return Err(Error::parse_at(
                "getelementptr constant expression needs at least one index",
                span,
            ));
        }
        Ok(self.module.add_constant(Constant::new(
            result_ty,
            ConstKind::Expr(ConstExpr::Gep {
                inbounds,
                source_ty,
                ptr,
                indices,
            }),
        )))
    }

    fn parse_const_cast(&mut self, result_ty: TypeId, op: CastOp, span: Span) -> Result<ConstId> {
        self.expect_punct('(')?;
        let value_ty = self.parse_type()?;
        let value = self.parse_constant(value_ty)?;
        self.expect_ident_is("to")?;
        let declared_ty = self.parse_type()?;
        self.expect_punct(')')?;
        if declared_ty != result_ty {
            return Err(Error::parse_at(
                "constant expression result type does not match its use",
                span,
            ));
        }
        Ok(self.module.add_constant(Constant::new(
            result_ty,
            ConstKind::Expr(ConstExpr::Cast {
                op,
                value: Operand::constant(value_ty, value),
            }),
        )))
    }

    fn parse_struct_constant(&mut self, ty: TypeId) -> Result<ConstId> {
        if self.module.types.struct_body(ty).is_none() {
            return Err(self.error("struct constant used with a non-struct type"));
        }
        let mut elems = Vec::new();
        if !self.eat_punct('}') {
            let mut index = 0;
            loop {
                let _ = self.parse_type()?;
                let field_ty = self
                    .module
                    .types
                    .struct_body(ty)
                    .and_then(|(fields, _)| fields.get(index).copied())
                    .unwrap_or(ty);
                elems.push(self.parse_constant(field_ty)?);
                index += 1;
                if !self.eat_punct(',') {
                    break;
                }
            }
            self.expect_punct('}')?;
        }
        Ok(self.module.const_aggregate(ty, elems))
    }

    fn parse_value(&mut self, function: &mut Function, ty: TypeId) -> Result<Operand> {
        if matches!(self.peek_kind(), TokenKind::Local(_)) {
            let (name, _) = self.expect_local()?;
            let symbol = self.module.intern(&name);
            let local = *self
                .local_names
                .get(&symbol)
                .ok_or_else(|| self.error(format!("use of undefined local value %{name}")))?;
            return Ok(Operand::local(function.local_type(local), local));
        }
        if matches!(self.peek_kind(), TokenKind::Global(_)) {
            let (name, span, quoted) = self.expect_global()?;
            let symbol = self.reference_symbol(&name, quoted, span)?;
            return Ok(if let Some(id) = self.module.function_named(symbol) {
                Operand::function(ty, id)
            } else {
                let id = self.module.global_or_insert(symbol);
                Operand::global(ty, id)
            });
        }
        let id = self.parse_constant(ty)?;
        Ok(Operand::constant(ty, id))
    }

    fn parse_value_forward(&mut self, function: &mut Function, ty: TypeId) -> Result<Operand> {
        if matches!(self.peek_kind(), TokenKind::Local(_)) {
            let (name, _) = self.expect_local()?;
            let symbol = self.module.intern(&name);
            let local = match self.local_names.get(&symbol).copied() {
                Some(local) => local,
                None => {
                    let local = function.forward_local(symbol, ty);
                    self.local_names.insert(symbol, local);
                    local
                }
            };
            if function.local_type(local) == INVALID_TYPE {
                function.locals[local as usize].ty = ty;
            }
            return Ok(Operand::local(function.local_type(local), local));
        }
        self.parse_value(function, ty)
    }

    fn parse_phi_value(&mut self, function: &mut Function, ty: TypeId) -> Result<Operand> {
        if matches!(self.peek_kind(), TokenKind::Local(_)) {
            let (name, _) = self.expect_local()?;
            let symbol = self.module.intern(&name);
            let local = match self.local_names.get(&symbol).copied() {
                Some(local) => local,
                None => {
                    let local = function.forward_local(symbol, ty);
                    self.local_names.insert(symbol, local);
                    local
                }
            };
            return Ok(Operand::local(function.local_type(local), local));
        }
        self.parse_value(function, ty)
    }

    fn parse_phi_block(&mut self, function: &mut Function) -> Result<BlockId> {
        let name = if matches!(self.peek_kind(), TokenKind::Local(_)) {
            self.expect_local()?.0
        } else if let TokenKind::Int(value) = self.peek_kind() {
            let name = Cow::Owned(value.to_string());
            self.advance();
            name
        } else {
            return Err(self.error("expected a phi predecessor block"));
        };
        let symbol = self.module.intern(&name);
        Ok(function.block_or_insert(symbol))
    }

    fn parse_block_ref(&mut self, function: &mut Function) -> Result<BlockId> {
        self.expect_ident_is("label")?;
        let name = if matches!(self.peek_kind(), TokenKind::Local(_)) {
            self.expect_local()?.0
        } else if let TokenKind::Int(value) = self.peek_kind() {
            let name = Cow::Owned(value.to_string());
            self.advance();
            name
        } else {
            return Err(self.error("expected a basic block reference"));
        };
        let symbol = self.module.intern(&name);
        Ok(function.block_or_insert(symbol))
    }

    fn parse_callee(&mut self, function: &mut Function) -> Result<Operand> {
        match self.peek_kind().clone() {
            TokenKind::Ident(text) if &*text == "asm" => {
                let constant = self.parse_inline_asm(INVALID_TYPE)?;
                let ty = self.module.types.pointer();
                Ok(Operand::constant(ty, constant))
            }
            TokenKind::Global(name) => {
                let span = self.peek().span;
                let quoted = self.peek().quoted;
                self.advance();
                let symbol = self.reference_symbol(&name, quoted, span)?;
                if let Some(id) = self.module.function_named(symbol) {
                    Ok(Operand::function(self.module.functions[id].ty, id))
                } else if let Some(id) = self.module.global_named(symbol) {
                    Ok(Operand::global(self.module.globals[id].ty, id))
                } else {
                    let id = self.module.function_or_insert(symbol);
                    Ok(Operand::function(INVALID_TYPE, id))
                }
            }
            TokenKind::Local(_) => {
                let (name, _) = self.expect_local()?;
                let symbol = self.module.intern(&name);
                let local = match self.local_names.get(&symbol).copied() {
                    Some(local) => local,
                    None => {
                        let local = function.forward_local(symbol, INVALID_TYPE);
                        self.local_names.insert(symbol, local);
                        local
                    }
                };
                Ok(Operand::local(function.local_type(local), local))
            }
            _ => {
                let ty = self.parse_type()?;
                self.parse_value(function, ty)
            }
        }
    }

    fn parse_inline_asm(&mut self, fn_ty: TypeId) -> Result<ConstId> {
        self.expect_ident_is("asm")?;
        let mut sideeffect = false;
        let mut alignstack = false;
        let mut inteldialect = false;
        let mut unwind = false;
        loop {
            match self.peek_kind() {
                TokenKind::Ident(text) if &**text == "sideeffect" => {
                    sideeffect = true;
                    self.advance();
                }
                TokenKind::Ident(text) if &**text == "alignstack" => {
                    alignstack = true;
                    self.advance();
                }
                TokenKind::Ident(text) if &**text == "inteldialect" => {
                    inteldialect = true;
                    self.advance();
                }
                TokenKind::Ident(text) if &**text == "unwind" => {
                    unwind = true;
                    self.advance();
                }
                _ => break,
            }
        }
        let asm = self.expect_string_bytes()?;
        self.expect_punct(',')?;
        let constraints = self.expect_string_bytes()?;
        let ty = self.module.types.pointer();
        Ok(self.module.add_constant(Constant::new(
            ty,
            ConstKind::InlineAsm {
                fn_ty,
                asm,
                constraints,
                sideeffect,
                alignstack,
                inteldialect,
                unwind,
            },
        )))
    }

    fn parse_instruction(&mut self, function: &mut Function) -> Result<(InstKind, Option<TypeId>)> {
        let mnemonic = match self.peek_kind() {
            TokenKind::Ident(text) => text.clone(),
            other => return Err(self.error(format!("expected an instruction, found {other:?}"))),
        };
        if let Some(op) = BinOp::parse(&mnemonic) {
            self.advance();
            let mut flags = IntFlags::default();
            let mut fastmath = FastMath::default();
            if op.is_float() {
                fastmath = self.parse_fastmath()?;
            } else {
                loop {
                    match self.peek_kind() {
                        TokenKind::Ident(text) if &**text == "nsw" => {
                            flags.nsw = true;
                            self.advance();
                        }
                        TokenKind::Ident(text) if &**text == "nuw" => {
                            flags.nuw = true;
                            self.advance();
                        }
                        TokenKind::Ident(text) if &**text == "exact" => {
                            flags.exact = true;
                            self.advance();
                        }
                        _ => break,
                    }
                }
            }
            let ty = self.parse_type()?;
            let lhs = self.parse_value(function, ty)?;
            self.expect_punct(',')?;
            let rhs = self.parse_value(function, ty)?;
            return Ok((
                InstKind::Binary {
                    op,
                    flags,
                    fastmath,
                    lhs,
                    rhs,
                },
                Some(ty),
            ));
        }
        match &*mnemonic {
            "fneg" => {
                self.advance();
                let flags = self.parse_fastmath()?;
                let ty = self.parse_type()?;
                let value = self.parse_value(function, ty)?;
                Ok((InstKind::FNeg { flags, value }, Some(ty)))
            }
            "icmp" => {
                self.advance();
                let pred_text = self.expect_ident()?;
                let pred = IcmpPred::parse(&pred_text)
                    .ok_or_else(|| self.error(format!("unknown icmp predicate `{pred_text}`")))?;
                let ty = self.parse_type()?;
                let lhs = self.parse_value(function, ty)?;
                self.expect_punct(',')?;
                let rhs = self.parse_value(function, ty)?;
                let result_ty = self.comparison_result_type(ty);
                Ok((InstKind::ICmp { pred, lhs, rhs }, Some(result_ty)))
            }
            "fcmp" => {
                self.advance();
                let flags = self.parse_fastmath()?;
                let pred_text = self.expect_ident()?;
                let pred = FcmpPred::parse(&pred_text)
                    .ok_or_else(|| self.error(format!("unknown fcmp predicate `{pred_text}`")))?;
                let ty = self.parse_type()?;
                let lhs = self.parse_value(function, ty)?;
                self.expect_punct(',')?;
                let rhs = self.parse_value(function, ty)?;
                let result_ty = self.comparison_result_type(ty);
                Ok((
                    InstKind::FCmp {
                        pred,
                        flags,
                        lhs,
                        rhs,
                    },
                    Some(result_ty),
                ))
            }
            "alloca" => {
                self.advance();
                let elem_ty = self.parse_type()?;
                let mut count = None;
                let mut align = None;
                if self.eat_punct(',') {
                    if self.eat_ident("align") {
                        align = Some(self.expect_int()? as u64);
                    } else {
                        let count_ty = self.parse_type()?;
                        count = Some(self.parse_value(function, count_ty)?);
                        if self.eat_punct(',') {
                            self.expect_ident_is("align")?;
                            align = Some(self.expect_int()? as u64);
                        }
                    }
                }
                let ptr = self.module.types.pointer();
                Ok((
                    InstKind::Alloca {
                        elem_ty,
                        count,
                        align,
                    },
                    Some(ptr),
                ))
            }
            "load" => {
                self.advance();
                let atomic = self.eat_ident("atomic");
                let volatile = self.eat_ident("volatile");
                let ty = self.parse_type()?;
                self.expect_punct(',')?;
                let ptr_ty = self.parse_type()?;
                let ptr = self.parse_value(function, ptr_ty)?;
                let (syncscope, ordering) = if atomic {
                    let syncscope = self.parse_syncscope()?;
                    let ordering = self.parse_atomic_ordering()?;
                    if !ordering.is_load_valid() {
                        return Err(self.error("invalid atomic load ordering"));
                    }
                    (syncscope, ordering)
                } else {
                    (SyncScope::System, AtomicOrdering::NotAtomic)
                };
                let align = self.parse_optional_align()?;
                if atomic && align.is_none() {
                    return Err(self.error("atomic load requires an explicit alignment"));
                }
                Ok((
                    InstKind::Load {
                        ty,
                        ptr,
                        align,
                        volatile,
                        ordering,
                        syncscope,
                    },
                    Some(ty),
                ))
            }
            "store" => {
                self.advance();
                let atomic = self.eat_ident("atomic");
                let volatile = self.eat_ident("volatile");
                let ty = self.parse_type()?;
                let value = self.parse_value(function, ty)?;
                self.expect_punct(',')?;
                let ptr_ty = self.parse_type()?;
                let ptr = self.parse_value(function, ptr_ty)?;
                let (syncscope, ordering) = if atomic {
                    let syncscope = self.parse_syncscope()?;
                    let ordering = self.parse_atomic_ordering()?;
                    if !ordering.is_store_valid() {
                        return Err(self.error("invalid atomic store ordering"));
                    }
                    (syncscope, ordering)
                } else {
                    (SyncScope::System, AtomicOrdering::NotAtomic)
                };
                let align = self.parse_optional_align()?;
                if atomic && align.is_none() {
                    return Err(self.error("atomic store requires an explicit alignment"));
                }
                Ok((
                    InstKind::Store {
                        value,
                        ptr,
                        align,
                        volatile,
                        ordering,
                        syncscope,
                    },
                    None,
                ))
            }
            "atomicrmw" => {
                self.advance();
                let volatile = self.eat_ident("volatile");
                let op_text = self.expect_ident()?;
                let op = AtomicRmwOp::parse(&op_text).ok_or_else(|| {
                    self.error(format!("unknown atomicrmw operation `{op_text}`"))
                })?;
                let ptr_ty = self.parse_type()?;
                let ptr = self.parse_value(function, ptr_ty)?;
                self.expect_punct(',')?;
                let ty = self.parse_type()?;
                let value = self.parse_value(function, ty)?;
                let syncscope = self.parse_syncscope()?;
                let ordering = self.parse_atomic_ordering()?;
                if !ordering.is_rmw_valid() {
                    return Err(self.error("invalid atomicrmw ordering"));
                }
                let align = self.parse_optional_align()?;
                if align.is_none() {
                    return Err(self.error("atomicrmw requires an explicit alignment"));
                }
                Ok((
                    InstKind::AtomicRmw {
                        op,
                        ptr,
                        value,
                        align,
                        volatile,
                        ordering,
                        syncscope,
                    },
                    Some(ty),
                ))
            }
            "cmpxchg" => {
                self.advance();
                let weak = self.eat_ident("weak");
                let volatile = self.eat_ident("volatile");
                let ptr_ty = self.parse_type()?;
                let ptr = self.parse_value(function, ptr_ty)?;
                self.expect_punct(',')?;
                let ty = self.parse_type()?;
                let cmp = self.parse_value(function, ty)?;
                self.expect_punct(',')?;
                let new_ty = self.parse_type()?;
                if new_ty != ty {
                    return Err(
                        self.error("cmpxchg compare and new values must have the same type")
                    );
                }
                let new = self.parse_value(function, new_ty)?;
                let syncscope = self.parse_syncscope()?;
                let success = self.parse_atomic_ordering()?;
                let failure = self.parse_atomic_ordering()?;
                if !success.is_cmpxchg_success_valid() {
                    return Err(self.error("invalid cmpxchg success ordering"));
                }
                if !failure.is_cmpxchg_failure_valid() {
                    return Err(self.error("invalid cmpxchg failure ordering"));
                }
                let align = self.parse_optional_align()?;
                if align.is_none() {
                    return Err(self.error("cmpxchg requires an explicit alignment"));
                }
                let i1 = self.module.types.int(1);
                let result_ty = self.module.types.intern(Type::Struct {
                    name: None,
                    fields: vec![ty, i1],
                    packed: false,
                });
                Ok((
                    InstKind::CmpXchg {
                        ptr,
                        cmp,
                        new,
                        align,
                        volatile,
                        success,
                        failure,
                        syncscope,
                        weak,
                    },
                    Some(result_ty),
                ))
            }
            "getelementptr" => {
                self.advance();
                let inbounds = self.eat_ident("inbounds");
                let source_ty = self.parse_type()?;
                self.expect_punct(',')?;
                let ptr_ty = self.parse_type()?;
                let ptr = self.parse_value(function, ptr_ty)?;
                let mut indices = Vec::new();
                while self.eat_punct(',') {
                    let idx_ty = self.parse_type()?;
                    let value = self.parse_value(function, idx_ty)?;
                    indices.push(GepIndex { ty: idx_ty, value });
                }
                let result_ty = self.module.types.pointer();
                Ok((
                    InstKind::Gep {
                        inbounds,
                        source_ty,
                        ptr,
                        indices,
                    },
                    Some(result_ty),
                ))
            }
            "call" => self.parse_call(function, TailKind::None),
            "callbr" => self.parse_callbr(function),
            "invoke" => self.parse_invoke(function),
            "landingpad" => self.parse_landingpad(function),
            "resume" => {
                self.advance();
                let ty = self.parse_type()?;
                let value = self.parse_value(function, ty)?;
                Ok((InstKind::Resume { value }, None))
            }
            "catchswitch" => self.parse_catchswitch(function),
            "catchpad" => self.parse_catchpad(function),
            "cleanuppad" => self.parse_cleanuppad(function),
            "catchret" => self.parse_catchret(function),
            "cleanupret" => self.parse_cleanupret(function),
            "tail" | "musttail" | "notail" => {
                let tail = match &*mnemonic {
                    "tail" => TailKind::Tail,
                    "musttail" => TailKind::MustTail,
                    _ => TailKind::NoTail,
                };
                self.advance();
                self.parse_call(function, tail)
            }
            "phi" => {
                self.advance();
                let ty = self.parse_type()?;
                let mut incoming = Vec::new();
                loop {
                    self.expect_punct('[')?;
                    let value = self.parse_phi_value(function, ty)?;
                    self.expect_punct(',')?;
                    let block = self.parse_phi_block(function)?;
                    self.expect_punct(']')?;
                    incoming.push((value, block));
                    if !self.eat_punct(',') {
                        break;
                    }
                }
                Ok((InstKind::Phi { incoming }, Some(ty)))
            }
            "select" => {
                self.advance();
                let fastmath = self.parse_fastmath()?;
                let cond_ty = self.parse_type()?;
                let cond = self.parse_value(function, cond_ty)?;
                self.expect_punct(',')?;
                let ty = self.parse_type()?;
                let on_true = self.parse_value(function, ty)?;
                self.expect_punct(',')?;
                let ty2 = self.parse_type()?;
                let on_false = self.parse_value(function, ty2)?;
                Ok((
                    InstKind::Select {
                        cond,
                        on_true,
                        on_false,
                        fastmath,
                    },
                    Some(ty),
                ))
            }
            "ret" => {
                self.advance();
                let value = if self.eat_ident("void") {
                    None
                } else {
                    let ty = self.parse_type()?;
                    Some(self.parse_value(function, ty)?)
                };
                Ok((InstKind::Ret { value }, None))
            }
            "br" => {
                self.advance();
                if self.at_ident("label") {
                    let dest = self.parse_block_ref(function)?;
                    Ok((InstKind::Br { dest }, None))
                } else {
                    let ty = self.parse_type()?;
                    let cond = self.parse_value(function, ty)?;
                    self.expect_punct(',')?;
                    let then_block = self.parse_block_ref(function)?;
                    self.expect_punct(',')?;
                    let else_block = self.parse_block_ref(function)?;
                    Ok((
                        InstKind::CondBr {
                            cond,
                            then_block,
                            else_block,
                        },
                        None,
                    ))
                }
            }
            "switch" => {
                self.advance();
                let ty = self.parse_type()?;
                let value = self.parse_value(function, ty)?;
                self.expect_punct(',')?;
                let default = self.parse_block_ref(function)?;
                self.expect_punct('[')?;
                let mut cases = Vec::new();
                self.skip_eol();
                if !self.eat_punct(']') {
                    loop {
                        self.skip_eol();
                        let case_ty = self.parse_type()?;
                        let case_value = self.parse_constant(case_ty)?;
                        self.expect_punct(',')?;
                        let block = self.parse_block_ref(function)?;
                        cases.push((case_value, block));
                        self.skip_eol();
                        if self.at_punct(']') {
                            break;
                        }
                    }
                    self.expect_punct(']')?;
                }
                Ok((
                    InstKind::Switch {
                        value,
                        default,
                        cases,
                    },
                    None,
                ))
            }
            "extractvalue" => {
                self.advance();
                let aggregate_ty = self.parse_type()?;
                let aggregate = self.parse_value_forward(function, aggregate_ty)?;
                let aggregate_ty =
                    self.require_operand_type(aggregate, aggregate_ty, "extractvalue aggregate")?;
                let indices = self.parse_aggregate_indices()?;
                let result_ty = self
                    .module
                    .types
                    .aggregate_indexed(aggregate_ty, &indices)
                    .ok_or_else(|| self.error("invalid extractvalue indices"))?;
                Ok((
                    InstKind::ExtractValue { aggregate, indices },
                    Some(result_ty),
                ))
            }
            "insertvalue" => {
                self.advance();
                let aggregate_ty = self.parse_type()?;
                let aggregate = self.parse_value_forward(function, aggregate_ty)?;
                let aggregate_ty =
                    self.require_operand_type(aggregate, aggregate_ty, "insertvalue aggregate")?;
                self.expect_punct(',')?;
                let value_ty = self.parse_type()?;
                let value = self.parse_value_forward(function, value_ty)?;
                let value_ty = self.require_operand_type(value, value_ty, "insertvalue value")?;
                let indices = self.parse_aggregate_indices()?;
                let expected = self
                    .module
                    .types
                    .aggregate_indexed(aggregate_ty, &indices)
                    .ok_or_else(|| self.error("invalid insertvalue indices"))?;
                if expected != value_ty {
                    return Err(self.error("insertvalue element type does not match the index"));
                }
                Ok((
                    InstKind::InsertValue {
                        aggregate,
                        value,
                        indices,
                    },
                    Some(aggregate_ty),
                ))
            }
            "extractelement" => {
                self.advance();
                let vector_ty = self.parse_type()?;
                let vector = self.parse_value_forward(function, vector_ty)?;
                let vector_ty =
                    self.require_operand_type(vector, vector_ty, "extractelement vector")?;
                self.expect_punct(',')?;
                let index_ty = self.parse_type()?;
                let index = self.parse_value_forward(function, index_ty)?;
                let index_ty =
                    self.require_operand_type(index, index_ty, "extractelement index")?;
                if !matches!(self.module.types.get(index_ty), Type::Int(_)) {
                    return Err(self.error("extractelement index is not an integer"));
                }
                let result_ty = self
                    .module
                    .types
                    .vector_element(vector_ty)
                    .ok_or_else(|| self.error("extractelement requires a vector operand"))?;
                Ok((InstKind::ExtractElement { vector, index }, Some(result_ty)))
            }
            "insertelement" => {
                self.advance();
                let vector_ty = self.parse_type()?;
                let vector = self.parse_value_forward(function, vector_ty)?;
                let vector_ty =
                    self.require_operand_type(vector, vector_ty, "insertelement vector")?;
                self.expect_punct(',')?;
                let value_ty = self.parse_type()?;
                let value = self.parse_value_forward(function, value_ty)?;
                let value_ty = self.require_operand_type(value, value_ty, "insertelement value")?;
                self.expect_punct(',')?;
                let index_ty = self.parse_type()?;
                let index = self.parse_value_forward(function, index_ty)?;
                let index_ty = self.require_operand_type(index, index_ty, "insertelement index")?;
                if !matches!(self.module.types.get(index_ty), Type::Int(_)) {
                    return Err(self.error("insertelement index is not an integer"));
                }
                if self.module.types.vector_element(vector_ty) != Some(value_ty) {
                    return Err(self.error("insertelement value type does not match the vector"));
                }
                Ok((
                    InstKind::InsertElement {
                        vector,
                        value,
                        index,
                    },
                    Some(vector_ty),
                ))
            }
            "shufflevector" => {
                self.advance();
                let first_ty = self.parse_type()?;
                let first = self.parse_value_forward(function, first_ty)?;
                let first_ty =
                    self.require_operand_type(first, first_ty, "shufflevector first vector")?;
                self.expect_punct(',')?;
                let second_ty = self.parse_type()?;
                let second = self.parse_value_forward(function, second_ty)?;
                let second_ty =
                    self.require_operand_type(second, second_ty, "shufflevector second vector")?;
                self.expect_punct(',')?;
                let mask_ty = self.parse_type()?;
                let mask = self.parse_value(function, mask_ty)?;
                let mask_ty = self.require_operand_type(mask, mask_ty, "shufflevector mask")?;
                if !matches!(mask.kind, OperandKind::Const(_)) {
                    return Err(self.error("shufflevector mask must be a constant"));
                }
                let mask = self.canonicalize_shuffle_mask(mask);
                let (first_elem, _, first_scalable) = self
                    .module
                    .types
                    .vector_info(first_ty)
                    .ok_or_else(|| self.error("shufflevector first operand is not a vector"))?;
                if first_scalable {
                    return Err(self.error("scalable shufflevector is not supported"));
                }
                if second_ty != first_ty {
                    return Err(self.error("shufflevector operands must have the same type"));
                }
                let (mask_elem, mask_len, mask_scalable) =
                    self.module
                        .types
                        .vector_info(mask_ty)
                        .ok_or_else(|| self.error("shufflevector mask is not a vector"))?;
                if mask_scalable {
                    return Err(self.error("scalable shufflevector masks are not supported"));
                }
                if !matches!(self.module.types.get(mask_elem), Type::Int(32)) {
                    return Err(self.error("shufflevector mask element type must be i32"));
                }
                let result_ty = self.module.types.vector(first_elem, mask_len, false);
                Ok((
                    InstKind::ShuffleVector {
                        first,
                        second,
                        mask,
                    },
                    Some(result_ty),
                ))
            }
            "freeze" => {
                self.advance();
                let ty = self.parse_type()?;
                let value = self.parse_value_forward(function, ty)?;
                let ty = self.require_operand_type(value, ty, "freeze value")?;
                Ok((InstKind::Freeze { value }, Some(ty)))
            }
            "fence" => {
                self.advance();
                let syncscope = self.parse_syncscope()?;
                let ordering = self.parse_atomic_ordering()?;
                if !ordering.is_fence_valid() {
                    return Err(self.error("fence requires acquire, release, acq_rel, or seq_cst"));
                }
                Ok((
                    InstKind::Fence {
                        ordering,
                        syncscope,
                    },
                    None,
                ))
            }
            "va_arg" => {
                self.advance();
                let list_ty = self.parse_type()?;
                let list = self.parse_value(function, list_ty)?;
                if !matches!(self.module.types.get(list_ty), Type::Pointer) {
                    return Err(self.error("va_arg list is not a pointer"));
                }
                self.expect_punct(',')?;
                let ty = self.parse_type()?;
                Ok((InstKind::VaArg { list, ty }, Some(ty)))
            }
            "indirectbr" => {
                self.advance();
                let address_ty = self.parse_type()?;
                let address = self.parse_value(function, address_ty)?;
                if !matches!(self.module.types.get(address_ty), Type::Pointer) {
                    return Err(self.error("indirectbr address is not a pointer"));
                }
                self.expect_punct(',')?;
                self.expect_punct('[')?;
                let mut destinations = Vec::new();
                if !self.eat_punct(']') {
                    loop {
                        destinations.push(self.parse_block_ref(function)?);
                        if !self.eat_punct(',') {
                            break;
                        }
                    }
                    self.expect_punct(']')?;
                }
                Ok((
                    InstKind::IndirectBr {
                        address,
                        destinations,
                    },
                    None,
                ))
            }
            "unreachable" => {
                self.advance();
                Ok((InstKind::Unreachable, None))
            }
            other => {
                if let Some(op) = CastOp::parse(other) {
                    self.advance();
                    let ty = self.parse_type()?;
                    let value = self.parse_value(function, ty)?;
                    self.expect_ident_is("to")?;
                    let to = self.parse_type()?;
                    Ok((InstKind::Cast { op, value, to }, Some(to)))
                } else {
                    Err(self.error(format!("unsupported instruction `{other}`")))
                }
            }
        }
    }

    fn canonicalize_shuffle_mask(&mut self, mask: Operand) -> Operand {
        let OperandKind::Const(id) = mask.kind else {
            return mask;
        };
        let ty = mask.ty;
        let kind = self.module.constant(id).kind.clone();
        match kind {
            ConstKind::Undef => Operand::constant(ty, self.module.const_poison(ty)),
            ConstKind::Aggregate(elems) => {
                let mut changed = false;
                let mut canonical = Vec::with_capacity(elems.len());
                for elem in elems {
                    let elem_ty = self.module.constant(elem).ty;
                    if matches!(self.module.constant(elem).kind, ConstKind::Undef) {
                        canonical.push(self.module.const_poison(elem_ty));
                        changed = true;
                    } else {
                        canonical.push(elem);
                    }
                }
                if changed {
                    Operand::constant(ty, self.module.const_aggregate(ty, canonical))
                } else {
                    mask
                }
            }
            _ => mask,
        }
    }

    fn comparison_result_type(&mut self, ty: TypeId) -> TypeId {
        let vector = match self.module.types.get(ty) {
            Type::Vector { len, scalable, .. } => Some((*len, *scalable)),
            _ => None,
        };
        let i1 = self.module.types.int(1);
        match vector {
            Some((len, scalable)) => self.module.types.vector(i1, len, scalable),
            None => i1,
        }
    }

    fn parse_syncscope(&mut self) -> Result<SyncScope> {
        if !self.eat_ident("syncscope") {
            return Ok(SyncScope::System);
        }
        self.expect_punct('(')?;
        let name = self.expect_string()?;
        self.expect_punct(')')?;
        match name.as_str() {
            "system" => Ok(SyncScope::System),
            "singlethread" => Ok(SyncScope::SingleThread),
            _ => Err(self.error(format!("unsupported syncscope `{name}`"))),
        }
    }

    fn parse_atomic_ordering(&mut self) -> Result<AtomicOrdering> {
        let text = self.expect_ident()?;
        AtomicOrdering::parse(&text)
            .ok_or_else(|| self.error(format!("unknown atomic ordering `{text}`")))
    }

    fn resolved_operand_type(&mut self, operand: Operand) -> TypeId {
        match operand.kind {
            OperandKind::Global(_) | OperandKind::Function(_) => self.module.types.pointer(),
            _ => operand.ty,
        }
    }

    fn require_operand_type(
        &mut self,
        operand: Operand,
        written: TypeId,
        what: &str,
    ) -> Result<TypeId> {
        let actual = self.resolved_operand_type(operand);
        if actual != written {
            return Err(self.error(format!("{what} type does not match the operand")));
        }
        Ok(actual)
    }

    fn parse_aggregate_indices(&mut self) -> Result<Vec<u32>> {
        let mut indices = Vec::new();
        loop {
            self.skip_eol();
            if !self.eat_punct(',') {
                break;
            }
            self.skip_eol();
            let index = self.expect_int()?;
            indices.push(
                u32::try_from(index).map_err(|_| self.error("aggregate index is out of range"))?,
            );
        }
        if indices.is_empty() {
            return Err(self.error("aggregate instruction requires at least one index"));
        }
        Ok(indices)
    }

    fn parse_call(
        &mut self,
        function: &mut Function,
        tail: TailKind,
    ) -> Result<(InstKind, Option<TypeId>)> {
        let call = self.parse_call_common(function, "call")?;
        let result_ty = if self.module.types.get(call.ret_ty).is_void() {
            None
        } else {
            Some(call.ret_ty)
        };
        Ok((
            InstKind::Call {
                tail,
                ret_ty: call.ret_ty,
                fn_ty: call.fn_ty,
                callee: call.callee,
                args: call.args,
                arg_attrs: call.arg_attrs,
                fastmath: call.fastmath,
                ret_attrs: call.ret_attrs.into(),
                attrs: call.attrs.into(),
            },
            result_ty,
        ))
    }

    fn parse_callbr(&mut self, function: &mut Function) -> Result<(InstKind, Option<TypeId>)> {
        let call = self.parse_call_common(function, "callbr")?;
        self.skip_eol();
        self.expect_ident_is("to")?;
        let default_dest = self.parse_block_ref(function)?;
        self.expect_punct('[')?;
        let mut indirect_dests = Vec::new();
        if self.eat_punct(']') {
            return Err(self.error("callbr requires at least one indirect destination"));
        }
        loop {
            indirect_dests.push(self.parse_block_ref(function)?);
            if !self.eat_punct(',') {
                break;
            }
        }
        self.expect_punct(']')?;
        let result_ty = if self.module.types.get(call.ret_ty).is_void() {
            None
        } else {
            Some(call.ret_ty)
        };
        Ok((
            InstKind::CallBr {
                ret_ty: call.ret_ty,
                fn_ty: call.fn_ty,
                callee: call.callee,
                args: call.args,
                arg_attrs: call.arg_attrs,
                fastmath: call.fastmath,
                ret_attrs: call.ret_attrs.into(),
                attrs: call.attrs.into(),
                default_dest,
                indirect_dests,
            },
            result_ty,
        ))
    }

    fn parse_invoke(&mut self, function: &mut Function) -> Result<(InstKind, Option<TypeId>)> {
        let call = self.parse_call_common(function, "invoke")?;
        self.skip_eol();
        self.expect_ident_is("to")?;
        let normal_dest = self.parse_block_ref(function)?;
        self.skip_eol();
        self.expect_ident_is("unwind")?;
        let unwind_dest = self.parse_block_ref(function)?;
        let result_ty = if self.module.types.get(call.ret_ty).is_void() {
            None
        } else {
            Some(call.ret_ty)
        };
        Ok((
            InstKind::Invoke {
                ret_ty: call.ret_ty,
                fn_ty: call.fn_ty,
                callee: call.callee,
                args: call.args,
                arg_attrs: call.arg_attrs,
                fastmath: call.fastmath,
                ret_attrs: call.ret_attrs.into(),
                attrs: call.attrs.into(),
                normal_dest,
                unwind_dest,
            },
            result_ty,
        ))
    }

    fn parse_landingpad(&mut self, function: &mut Function) -> Result<(InstKind, Option<TypeId>)> {
        self.advance();
        let ty = self.parse_type()?;
        self.skip_eol();
        let cleanup = self.eat_ident("cleanup");
        let mut clauses = Vec::new();
        loop {
            self.skip_eol();
            let clause = if self.eat_ident("catch") {
                LandingPadClause::Catch
            } else if self.eat_ident("filter") {
                LandingPadClause::Filter
            } else {
                break;
            };
            let clause_ty = self.parse_type()?;
            let value = self.parse_value(function, clause_ty)?;
            clauses.push((clause, value));
        }
        Ok((
            InstKind::LandingPad {
                ty,
                cleanup,
                clauses,
            },
            Some(ty),
        ))
    }

    fn parse_parent_pad(&mut self, function: &mut Function) -> Result<Option<Operand>> {
        self.skip_eol();
        self.expect_ident_is("within")?;
        self.skip_eol();
        if self.eat_ident("none") {
            return Ok(None);
        }
        let ty = self.module.types.intern(Type::Token);
        Ok(Some(self.parse_value_forward(function, ty)?))
    }

    fn parse_unwind_dest(&mut self, function: &mut Function) -> Result<Option<BlockId>> {
        self.skip_eol();
        self.expect_ident_is("unwind")?;
        self.skip_eol();
        if self.eat_ident("to") {
            self.skip_eol();
            self.expect_ident_is("caller")?;
            return Ok(None);
        }
        Ok(Some(self.parse_block_ref(function)?))
    }

    fn parse_catchswitch(&mut self, function: &mut Function) -> Result<(InstKind, Option<TypeId>)> {
        self.advance();
        let parent_pad = self.parse_parent_pad(function)?;
        self.skip_eol();
        self.expect_punct('[')?;
        let mut handlers = Vec::new();
        loop {
            self.skip_eol();
            if self.eat_punct(']') {
                break;
            }
            handlers.push(self.parse_block_ref(function)?);
            self.skip_eol();
            if self.eat_punct(']') {
                break;
            }
            self.expect_punct(',')?;
        }
        let unwind_dest = self.parse_unwind_dest(function)?;
        let ty = self.module.types.intern(Type::Token);
        Ok((
            InstKind::CatchSwitch {
                parent_pad,
                handlers,
                unwind_dest,
            },
            Some(ty),
        ))
    }

    fn parse_pad_args(&mut self, function: &mut Function) -> Result<Vec<Operand>> {
        self.skip_eol();
        self.expect_punct('[')?;
        let mut args = Vec::new();
        loop {
            self.skip_eol();
            if self.eat_punct(']') {
                break;
            }
            let ty = self.parse_type()?;
            args.push(self.parse_value_forward(function, ty)?);
            self.skip_eol();
            if self.eat_punct(']') {
                break;
            }
            self.expect_punct(',')?;
        }
        Ok(args)
    }

    fn parse_catchpad(&mut self, function: &mut Function) -> Result<(InstKind, Option<TypeId>)> {
        self.advance();
        let parent_pad = self.parse_parent_pad(function)?;
        let args = self.parse_pad_args(function)?;
        let ty = self.module.types.intern(Type::Token);
        Ok((InstKind::CatchPad { parent_pad, args }, Some(ty)))
    }

    fn parse_cleanuppad(&mut self, function: &mut Function) -> Result<(InstKind, Option<TypeId>)> {
        self.advance();
        let parent_pad = self.parse_parent_pad(function)?;
        let args = self.parse_pad_args(function)?;
        let ty = self.module.types.intern(Type::Token);
        Ok((InstKind::CleanupPad { parent_pad, args }, Some(ty)))
    }

    fn parse_catchret(&mut self, function: &mut Function) -> Result<(InstKind, Option<TypeId>)> {
        self.advance();
        self.skip_eol();
        self.expect_ident_is("from")?;
        let ty = self.module.types.intern(Type::Token);
        let catch_pad = self.parse_value_forward(function, ty)?;
        self.skip_eol();
        self.expect_ident_is("to")?;
        let successor = self.parse_block_ref(function)?;
        Ok((
            InstKind::CatchRet {
                catch_pad,
                successor,
            },
            None,
        ))
    }

    fn parse_cleanupret(&mut self, function: &mut Function) -> Result<(InstKind, Option<TypeId>)> {
        self.advance();
        self.skip_eol();
        self.expect_ident_is("from")?;
        let ty = self.module.types.intern(Type::Token);
        let cleanup_pad = self.parse_value_forward(function, ty)?;
        let unwind_dest = self.parse_unwind_dest(function)?;
        Ok((
            InstKind::CleanupRet {
                cleanup_pad,
                unwind_dest,
            },
            None,
        ))
    }

    fn parse_call_common(&mut self, function: &mut Function, mnemonic: &str) -> Result<ParsedCall> {
        self.expect_ident_is(mnemonic)?;
        let fastmath = self.parse_fastmath()?;
        let mut attrs = Vec::new();
        if let TokenKind::Ident(text) = self.peek_kind() {
            if &**text == "cc" {
                self.advance();
                let value = self.expect_int()?;
                attrs.push(Attribute(format!("cc {value}")));
            } else if matches!(
                &**text,
                "fastcc" | "coldcc" | "swiftcc" | "tailcc" | "preserve_mostcc" | "preserve_allcc"
            ) {
                attrs.push(Attribute(text.to_string()));
                self.advance();
            }
        }
        let mut ret_attrs = Vec::new();
        loop {
            match self.peek_kind().clone() {
                TokenKind::Ident(text) if is_attr_keyword(&text) => {
                    ret_attrs.push(self.parse_attribute()?);
                }
                TokenKind::Str(_) => ret_attrs.push(self.parse_attribute_token()?),
                _ => break,
            }
        }
        let ty = self.parse_type()?;
        let ret_ty = match self.module.types.get(ty) {
            Type::Function { ret, .. } => *ret,
            _ => ty,
        };
        let callee = self.parse_callee(function)?;
        self.expect_punct('(')?;
        let mut args = Vec::new();
        let mut arg_attrs = Vec::new();
        if !self.at_punct(')') {
            loop {
                let arg_ty = self.parse_type()?;
                arg_attrs.push(self.parse_param_attrs()?);
                args.push(self.parse_value(function, arg_ty)?);
                if !self.eat_punct(',') {
                    break;
                }
            }
        }
        self.expect_punct(')')?;
        self.parse_call_attrs(&mut attrs)?;
        let fn_ty = if matches!(self.module.types.get(ty), Type::Function { .. }) {
            ty
        } else if let OperandKind::Function(id) = callee.kind {
            if self.module.types.is_valid(self.module.functions[id].ty) {
                self.module.functions[id].ty
            } else {
                let params = args.iter().map(|arg| arg.ty).collect();
                self.module.types.function(ret_ty, params, false)
            }
        } else {
            let params = args.iter().map(|arg| arg.ty).collect();
            self.module.types.function(ret_ty, params, false)
        };
        if let OperandKind::Const(id) = callee.kind {
            if let ConstKind::InlineAsm {
                fn_ty: asm_fn_ty, ..
            } = &mut self.module.constants[id].kind
            {
                *asm_fn_ty = fn_ty;
            }
        }
        Ok(ParsedCall {
            fastmath,
            attrs,
            ret_attrs,
            ret_ty,
            fn_ty,
            callee,
            args,
            arg_attrs: arg_attrs.into_iter().map(Arc::new).collect(),
        })
    }

    fn parse_call_attrs(&mut self, attrs: &mut Vec<Attribute>) -> Result<()> {
        loop {
            match self.peek_kind().clone() {
                TokenKind::Eol | TokenKind::Eof => return Ok(()),
                TokenKind::Punct('}') => return Ok(()),
                TokenKind::AttrGroup(value) => {
                    self.advance();
                    attrs.push(Attribute(format!("#{value}")));
                }
                TokenKind::Str(_) => attrs.push(self.parse_attribute_token()?),
                TokenKind::Ident(text) if is_attr_keyword(&text) => {
                    attrs.push(self.parse_attribute()?);
                }
                TokenKind::Punct(',') => self.advance(),
                TokenKind::Meta(_) => {
                    self.advance();
                    if matches!(self.peek_kind(), TokenKind::Meta(_)) {
                        self.advance();
                    }
                }
                TokenKind::Punct('!') => self.skip_line(),
                _ => return Ok(()),
            }
        }
    }

    fn parse_fastmath(&mut self) -> Result<FastMath> {
        let mut flags = FastMath::default();
        loop {
            match self.peek_kind() {
                TokenKind::Ident(text) if flags.set(text) => self.advance(),
                _ => return Ok(flags),
            }
        }
    }

    fn parse_optional_align(&mut self) -> Result<Option<u64>> {
        if !self.eat_punct(',') {
            return Ok(None);
        }
        self.expect_ident_is("align")?;
        Ok(Some(self.expect_int()? as u64))
    }

    fn peek(&self) -> &Token<'a> {
        self.tokens.front().expect("parser always keeps a token")
    }

    fn peek_kind(&self) -> &TokenKind<'a> {
        &self.peek().kind
    }

    fn peek_kind_at(&self, offset: usize) -> &TokenKind<'a> {
        match self.tokens.get(offset) {
            Some(token) => &token.kind,
            None => {
                &self
                    .tokens
                    .back()
                    .expect("parser always keeps a token")
                    .kind
            }
        }
    }

    fn advance(&mut self) {
        if self.tokens.len() > 1 {
            self.tokens.pop_front();
        }
        self.ensure(2);
    }

    fn take(&mut self) -> Token<'a> {
        if self.tokens.len() > 1 {
            let token = self
                .tokens
                .pop_front()
                .expect("parser always keeps a token");
            self.ensure(2);
            token
        } else {
            self.peek().clone()
        }
    }

    fn at_eol(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eol)
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek_kind(), TokenKind::Eof)
    }

    fn at_punct(&self, ch: char) -> bool {
        matches!(self.peek_kind(), TokenKind::Punct(found) if *found == ch)
    }

    fn at_ident(&self, text: &str) -> bool {
        matches!(self.peek_kind(), TokenKind::Ident(found) if &**found == text)
    }

    fn eat_punct(&mut self, ch: char) -> bool {
        if self.at_punct(ch) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn eat_ident(&mut self, text: &str) -> bool {
        if self.at_ident(text) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn eat_ellipsis(&mut self) -> bool {
        if matches!(self.peek_kind(), TokenKind::Ellipsis) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect_punct(&mut self, ch: char) -> Result<()> {
        if self.eat_punct(ch) {
            Ok(())
        } else {
            Err(self.error(format!("expected `{ch}`")))
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        let token = self.take();
        match token.kind {
            TokenKind::Ident(text) => Ok(text.into_owned()),
            other => Err(Error::parse_at(
                format!("expected an identifier, found {other:?}"),
                token.span,
            )),
        }
    }

    fn expect_ident_is(&mut self, expected: &str) -> Result<()> {
        if self.at_ident(expected) {
            self.advance();
            return Ok(());
        }
        Err(self.error(format!(
            "expected `{expected}`, found {:?}",
            self.peek_kind()
        )))
    }

    fn expect_local(&mut self) -> Result<(Cow<'a, str>, Span)> {
        let token = self.take();
        match token.kind {
            TokenKind::Local(name) => Ok((name, token.span)),
            other => Err(Error::parse_at(
                format!("expected a local name, found {other:?}"),
                token.span,
            )),
        }
    }

    /// Resolves a definition's symbol and advances the shared global/function
    /// numbered-definition sequence. References do not call this.
    fn definition_symbol(
        &mut self,
        name: &str,
        quoted: bool,
        kind: &str,
        span: Span,
    ) -> Result<Symbol> {
        if !is_unnamed_symbol(name, quoted) {
            return Ok(self.module.intern(name));
        }
        if name.is_empty() {
            let slot = u32::try_from(self.next_unnamed)
                .map_err(|_| Error::parse_at(format!("too many unnamed {kind}s"), span))?;
            self.next_unnamed += 1;
            return Ok(self.unnamed_slot_symbol(slot));
        }
        let slot: u32 = name.parse().map_err(|_| {
            Error::parse_at(format!("invalid unnamed {kind} number `{name}`"), span)
        })?;
        if u64::from(slot) < self.next_unnamed {
            return Err(Error::parse_at(
                format!(
                    "{kind} expected to be numbered '@{}' or greater",
                    self.next_unnamed
                ),
                span,
            ));
        }
        self.next_unnamed = u64::from(slot) + 1;
        Ok(self.unnamed_slot_symbol(slot))
    }

    /// Resolves a forward reference without consuming a definition slot.
    fn reference_symbol(&mut self, name: &str, quoted: bool, span: Span) -> Result<Symbol> {
        if !is_unnamed_symbol(name, quoted) {
            return Ok(self.module.intern(name));
        }
        if name.is_empty() {
            return Err(Error::parse_at("use of undefined value '@'", span));
        }
        let slot: u32 = name.parse().map_err(|_| {
            Error::parse_at(format!("invalid unnamed symbol number `{name}`"), span)
        })?;
        Ok(self.unnamed_slot_symbol(slot))
    }

    fn unnamed_slot_symbol(&mut self, slot: u32) -> Symbol {
        if let Some(symbol) = self.unnamed_symbols.get(&slot) {
            return *symbol;
        }
        let symbol = self.module.intern(&format!("\0unnamed{slot}"));
        self.unnamed_symbols.insert(slot, symbol);
        symbol
    }

    fn expect_global(&mut self) -> Result<(Cow<'a, str>, Span, bool)> {
        let token = self.take();
        match token.kind {
            TokenKind::Global(name) => Ok((name, token.span, token.quoted)),
            other => Err(Error::parse_at(
                format!("expected a global name, found {other:?}"),
                token.span,
            )),
        }
    }

    fn expect_int(&mut self) -> Result<u128> {
        let token = self.take();
        match token.kind {
            TokenKind::Int(value) => Ok(value),
            other => Err(Error::parse_at(
                format!("expected an integer, found {other:?}"),
                token.span,
            )),
        }
    }

    fn expect_string(&mut self) -> Result<String> {
        match self.peek_kind().clone() {
            TokenKind::Str(bytes) => {
                self.advance();
                String::from_utf8(bytes)
                    .map_err(|_| self.error("string literal is not valid UTF-8"))
            }
            other => Err(self.error(format!("expected a string, found {other:?}"))),
        }
    }

    fn expect_string_bytes(&mut self) -> Result<Vec<u8>> {
        let token = self.take();
        match token.kind {
            TokenKind::Str(bytes) => Ok(bytes),
            other => Err(Error::parse_at(
                format!("expected a string, found {other:?}"),
                token.span,
            )),
        }
    }

    fn expect_eol(&mut self) -> Result<()> {
        if self.at_eol() {
            self.advance();
            Ok(())
        } else if self.at_eof() {
            Ok(())
        } else {
            Err(self.error("expected end of line"))
        }
    }

    fn skip_eol(&mut self) {
        while self.at_eol() {
            self.advance();
        }
    }

    fn skip_line(&mut self) {
        while !self.at_eol() && !self.at_eof() {
            self.advance();
        }
    }

    fn error(&self, message: impl Into<String>) -> Error {
        Error::parse_at(message, self.peek().span)
    }
}

fn mask_int(value: u128, bits: u32) -> u128 {
    if bits == 0 {
        0
    } else if bits >= 128 {
        value
    } else {
        value & ((1u128 << bits) - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{print_module, verify_module};

    #[test]
    fn lexes_attribute_groups() {
        let tokens = Lexer::new("attributes #0 = { nounwind }\n")
            .tokenize()
            .expect("lex");
        assert_eq!(
            tokens[0].kind,
            TokenKind::Ident(Cow::Borrowed("attributes"))
        );
        assert_eq!(tokens[1].kind, TokenKind::AttrGroup(0));
    }

    #[test]
    fn rejects_nonzero_address_spaces() {
        assert!(parse_module("@g = addrspace(1) global i32 0\n").is_err());
        let source = "define ptr @f(ptr addrspace(1) %p) {\nentry:\n  ret ptr %p\n}\n";
        assert!(parse_module(source).is_err());
    }

    #[test]
    fn forward_references_do_not_consume_definition_slots() {
        let source = r#"
@x = global ptr @1
@"" = global i32 11
@1 = global i32 22
@"" = global i32 33
@q = global ptr @2
"#;
        let module = parse_module(source).expect("parse");
        let q = module
            .globals
            .iter()
            .find_map(|(_, global)| (module.name(global.name) == "q").then_some(global))
            .expect("q global");
        let ConstKind::Global(target) = module.constant(q.init.expect("q initializer")).kind else {
            panic!("q should reference a global");
        };
        let target = &module.globals[target];
        let value = module.constant(target.init.expect("target initializer"));
        assert_eq!(value.kind, ConstKind::Int(33));
        assert!(print_module(&module).contains("@q = global ptr @2"));
    }

    #[test]
    fn rejects_reused_and_exhausted_numbered_slots() {
        let reused = parse_module("@0 = global i32 1\n@0 = global i32 2\n")
            .expect_err("reused slot must fail");
        assert!(
            reused
                .to_string()
                .contains("expected to be numbered '@1' or greater"),
            "{reused}"
        );

        let exhausted = parse_module(
            "@4294967294 = global i32 0\n@\"\" = global i32 1\n@\"\" = global i32 2\n",
        )
        .expect_err("exhausted slot space must fail");
        assert!(
            exhausted.to_string().contains("too many unnamed globals"),
            "{exhausted}"
        );

        let empty_reference =
            parse_module("@x = global ptr @\"\"\n").expect_err("empty reference must fail");
        assert!(
            empty_reference.to_string().contains("undefined value '@'"),
            "{empty_reference}"
        );
    }

    #[test]
    fn parses_quoted_type_attributes() {
        let source = r#"
%"a b" = type { i64 }
%ab = type { i8 }

declare void @take(ptr byval(%"a b"), ptr byval([8 x i8]))
declare void @other(ptr byval(%ab))
declare void @aligned() alignstack(16)

attributes #0 = { memory(none) alignstack=16 }
"#;
        let module = parse_module(source).expect("parse");
        assert!(verify_module(&module).is_ok());
        let printed = print_module(&module);
        assert!(printed.contains(r#"byval(%"a b")"#));
        assert!(printed.contains("byval([8 x i8])"));
        assert!(printed.contains("byval(%ab)"));
        assert!(printed.contains("alignstack(16)"));
        assert!(printed.contains("memory(none) alignstack=16"));
    }

    #[test]
    fn resolves_forward_global_reference() {
        let source = r#"
@g = global ptr @later
@later = global i32 7
"#;
        let module = parse_module(source).expect("parse");
        assert!(verify_module(&module).is_ok());
        let printed = print_module(&module);
        assert!(printed.contains("@g = global ptr @later"));
        assert!(printed.contains("@later = global i32 7"));
    }

    #[test]
    fn resolves_forward_function_operand() {
        let source = r#"
define ptr @ret_fn() {
entry:
  ret ptr @f
}

define void @f() {
entry:
  ret void
}
"#;
        let module = parse_module(source).expect("parse");
        assert!(verify_module(&module).is_ok());
        let printed = print_module(&module);
        assert!(printed.contains("ret ptr @f"), "{printed}");
    }

    #[test]
    fn preserves_group_align_and_raw_attribute_bytes() {
        let source = r#"
attributes #0 = { align=16 "key"="\FF" }

declare void @f() #0
"#;
        let module = parse_module(source).expect("parse");
        let printed = print_module(&module);
        assert!(printed.contains("align=16"), "{printed}");
        assert!(printed.contains(r#""key"="\FF""#), "{printed}");
    }

    #[test]
    fn rounds_half_and_bfloat_literals() {
        let source = r#"
@tiny_h = global half 1.0e-20
@below_h = global half 1.0014648437499998
@mid_h = global half 1.00146484375
@above_h = global half 1.0014648437500002
@tiny_b = global bfloat 1.0e-50
@below_b = global bfloat 1.0039062499999998
@mid_b = global bfloat 1.00390625
@above_b = global bfloat 1.0039062500000002
"#;
        let module = parse_module(source).expect("parse");
        let printed = print_module(&module);
        assert!(
            printed.contains("@tiny_h = global half 0xH0000"),
            "{printed}"
        );
        assert!(
            printed.contains("@below_h = global half 0xH3C01"),
            "{printed}"
        );
        assert!(
            printed.contains("@mid_h = global half 0xH3C02"),
            "{printed}"
        );
        assert!(
            printed.contains("@above_h = global half 0xH3C02"),
            "{printed}"
        );
        assert!(
            printed.contains("@tiny_b = global bfloat 0xR0000"),
            "{printed}"
        );
        assert!(
            printed.contains("@below_b = global bfloat 0xR3F80"),
            "{printed}"
        );
        assert!(
            printed.contains("@mid_b = global bfloat 0xR3F80"),
            "{printed}"
        );
        assert!(
            printed.contains("@above_b = global bfloat 0xR3F81"),
            "{printed}"
        );
    }

    #[test]
    fn parses_switch_cases() {
        let source = r#"
define i32 @g(i32 %x) {
entry:
  switch i32 %x, label %d [ i32 0, label %a i32 1, label %b ]
a:
  ret i32 1
b:
  ret i32 2
d:
  ret i32 3
}
"#;
        let module = parse_module(source).expect("parse");
        assert!(verify_module(&module).is_ok());
        let printed = print_module(&module);
        println!("{printed}");
        assert!(printed.contains("switch i32 %x, label %d [ i32 0, label %a i32 1, label %b ]"));
    }
}

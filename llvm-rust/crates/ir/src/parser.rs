//! Textual LLVM IR parser for the first Track B slice.
//!
//! Supported: module headers, named types, globals, declarations,
//! definitions, integer/float/aggregate constants, the core instruction set,
//! and raw attribute spellings. Metadata is accepted and skipped; MC, debug
//! info, and constant expressions are later milestones.

use std::collections::HashMap;

use llvm_rust_support::{Error, Result, Span, Symbol};

use crate::constant::{ConstId, ConstKind, Constant};
use crate::function::{BlockId, Function, LocalId};
use crate::inst::{
    Attribute, BinOp, CastOp, FastMath, FcmpPred, GepIndex, IcmpPred, InstKind, Instruction,
    IntFlags, TailKind,
};
use crate::linkage::{Linkage, Visibility};
use crate::module::{Global, Module};
use crate::names::quote_bytes;
use crate::ty::{FloatKind, INVALID_TYPE, Type, TypeId};
use crate::value::{Operand, OperandKind};

/// Parser error. Kept as an alias so callers can use the shared error type.
pub type ParseError = Error;

/// Parses one textual IR module.
pub fn parse_module(source: &str) -> core::result::Result<Module, ParseError> {
    let tokens = Lexer::new(source).tokenize()?;
    let mut parser = Parser::new(source, tokens);
    parser.parse_top_level()?;
    Ok(parser.module)
}

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
    Local(String),
    Global(String),
    Meta(String),
    AttrGroup(u32),
    Ident(String),
    Int(u128),
    Float(String),
    Str(Vec<u8>),
    CStr(Vec<u8>),
    Punct(char),
    Ellipsis,
    Eol,
    Eof,
}

#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    span: Span,
}

struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
    tokens: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            tokens: Vec::new(),
        }
    }

    fn tokenize(mut self) -> Result<Vec<Token>> {
        while self.pos < self.bytes.len() {
            let start = self.pos;
            let byte = self.bytes[self.pos];
            match byte {
                b' ' | b'\t' | b'\r' => self.pos += 1,
                b'\n' => {
                    self.pos += 1;
                    self.push(TokenKind::Eol, start);
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
                    self.push(kind, start);
                }
                b'!' => {
                    self.pos += 1;
                    if self.peek_byte() == Some(b'{') {
                        self.push(TokenKind::Punct('!'), start);
                    } else {
                        let name = self.read_sigil_name(start)?;
                        self.push(TokenKind::Meta(name), start);
                    }
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
                    self.push(TokenKind::AttrGroup(value), start);
                }
                b'"' => {
                    let bytes = self.read_quoted(start)?;
                    self.push(TokenKind::Str(bytes), start);
                }
                b'c' if self.peek_byte_at(1) == Some(b'"') => {
                    self.pos += 1;
                    let bytes = self.read_quoted(start)?;
                    self.push(TokenKind::CStr(bytes), start);
                }
                b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'<' | b'>' | b',' | b'=' | b':'
                | b'*' => {
                    self.pos += 1;
                    self.push(TokenKind::Punct(byte as char), start);
                }
                b'.' if self.bytes[self.pos..].starts_with(b"...") => {
                    self.pos += 3;
                    self.push(TokenKind::Ellipsis, start);
                }
                b'-' if self.peek_byte_at(1).is_some_and(|b| b.is_ascii_digit()) => {
                    self.read_number(start)?;
                }
                b'0'..=b'9' => self.read_number(start)?,
                b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' | b'.' | b'-' => {
                    let ident = self.read_while(is_ident_char);
                    self.push(TokenKind::Ident(ident), start);
                }
                _ => {
                    return Err(Error::parse_at(
                        format!("unexpected character `{}`", byte as char),
                        Span::new(start, start + 1),
                    ));
                }
            }
        }
        let end = self.bytes.len();
        self.push(TokenKind::Eof, end);
        Ok(self.tokens)
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        self.tokens.push(Token {
            kind,
            span: Span::new(start, self.pos),
        });
    }

    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_byte_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    fn read_while(&mut self, predicate: impl Fn(u8) -> bool) -> String {
        let start = self.pos;
        while self.pos < self.bytes.len() && predicate(self.bytes[self.pos]) {
            self.pos += 1;
        }
        self.source[start..self.pos].to_string()
    }

    fn read_sigil_name(&mut self, start: usize) -> Result<String> {
        if self.peek_byte() == Some(b'"') {
            let bytes = self.read_quoted(start)?;
            return String::from_utf8(bytes).map_err(|_| {
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
        Ok(name)
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

    fn read_number(&mut self, start: usize) -> Result<()> {
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
        let kind = if is_float_literal(&text) {
            TokenKind::Float(text)
        } else {
            let value = parse_int_literal(&text).ok_or_else(|| {
                Error::parse_at(
                    format!("invalid integer literal `{text}`"),
                    Span::new(start, self.pos),
                )
            })?;
            TokenKind::Int(value)
        };
        self.push(kind, start);
        Ok(())
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
        let rest = rest
            .strip_prefix('K')
            .or_else(|| rest.strip_prefix('L'))
            .or_else(|| rest.strip_prefix('M'))
            .or_else(|| rest.strip_prefix('H'))
            .or_else(|| rest.strip_prefix('R'))
            .unwrap_or(rest);
        let bits = u64::from_str_radix(rest, 16).ok()?;
        return Some(match kind {
            FloatKind::Half | FloatKind::BFloat => bits & 0xffff,
            FloatKind::Float => bits & 0xffff_ffff,
            _ => bits,
        });
    }
    let value = text.parse::<f64>().ok()?;
    Some(match kind {
        FloatKind::Half | FloatKind::BFloat => (f64_to_half(value) as u64) & 0xffff,
        FloatKind::Float => (value as f32).to_bits() as u64,
        FloatKind::Double => value.to_bits(),
        _ => value.to_bits(),
    })
}

fn f64_to_half(value: f64) -> u16 {
    let bits = (value as f32).to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if mantissa != 0 { 0x200 } else { 0 };
    }
    let unbiased = exp - 127 + 15;
    if unbiased >= 0x1f {
        return sign | 0x7c00;
    }
    if unbiased <= 0 {
        if unbiased < -10 {
            return sign;
        }
        let mantissa = mantissa | 0x80_0000;
        let shift = (14 - unbiased) as u32;
        return sign | (mantissa >> shift) as u16;
    }
    sign | ((unbiased as u16) << 10) | (mantissa >> 13) as u16
}

fn token_text(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Local(name) => format!("%{name}"),
        TokenKind::Global(name) => format!("@{name}"),
        TokenKind::Meta(name) => format!("!{name}"),
        TokenKind::AttrGroup(value) => format!("#{value}"),
        TokenKind::Ident(text) => text.clone(),
        TokenKind::Int(value) => value.to_string(),
        TokenKind::Float(text) => text.clone(),
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
            | "memory"
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

struct Parser<'a> {
    _source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    module: Module,
    local_names: HashMap<Symbol, LocalId>,
    next_local: u32,
    next_block: u32,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            _source: source,
            tokens,
            pos: 0,
            module: Module::new(),
            local_names: HashMap::new(),
            next_local: 0,
            next_block: 0,
        }
    }

    fn parse_top_level(&mut self) -> Result<()> {
        loop {
            self.skip_eol();
            if self.at_eof() {
                return Ok(());
            }
            match self.peek_kind().clone() {
                TokenKind::Ident(text) => match text.as_str() {
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
        let mut depth = 1usize;
        let mut text = String::new();
        while depth > 0 {
            if self.at_eof() {
                return Err(self.error("unterminated attribute group"));
            }
            let token = self.tokens[self.pos].clone();
            self.pos += 1;
            match token.kind {
                TokenKind::Punct('{') => {
                    depth += 1;
                    text.push('{');
                }
                TokenKind::Punct('}') => {
                    depth -= 1;
                    if depth > 0 {
                        text.push('}');
                    }
                }
                TokenKind::Punct('=') => {
                    while text.ends_with(' ') {
                        text.pop();
                    }
                    text.push('=');
                }
                other => {
                    if !text.is_empty() && !text.ends_with('=') {
                        text.push(' ');
                    }
                    text.push_str(&token_text(&other));
                }
            }
        }
        self.module.attr_groups.push((group, text));
        self.expect_eol()
    }

    fn parse_global(&mut self) -> Result<()> {
        let (name, _) = self.expect_global()?;
        let symbol = self.module.intern(&name);
        self.expect_punct('=')?;
        let mut linkage = Linkage::External;
        let mut visibility = Visibility::Default;
        let mut dso_local = false;
        let mut thread_local = false;
        let mut unnamed_addr = false;
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
            match text.as_str() {
                "dso_local" | "dso_preemptable" => {
                    dso_local = text == "dso_local";
                    self.advance();
                }
                "thread_local" => {
                    thread_local = true;
                    self.advance();
                    if self.eat_punct('(') {
                        self.skip_balanced(')')?;
                    }
                }
                "unnamed_addr" | "local_unnamed_addr" => {
                    unnamed_addr = true;
                    self.advance();
                }
                "externally_initialized" => {
                    self.advance();
                }
                "addrspace" => {
                    self.advance();
                    self.expect_punct('(')?;
                    let _ = self.expect_int()?;
                    self.expect_punct(')')?;
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
            thread_local,
            unnamed_addr,
            dso_local,
            align,
            section,
        };
        let id = self.module.global_or_insert(symbol);
        self.module.define_global(id, global);
        Ok(())
    }

    fn parse_function(&mut self, is_declaration: bool) -> Result<()> {
        self.advance();
        self.local_names.clear();
        self.next_local = 0;
        self.next_block = 0;
        let mut linkage = Linkage::External;
        let mut visibility = Visibility::Default;
        let mut dso_local = false;
        let mut unnamed_addr = false;
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
            match text.as_str() {
                "dso_local" | "dso_preemptable" => {
                    dso_local = text == "dso_local";
                    self.advance();
                }
                "unnamed_addr" | "local_unnamed_addr" => {
                    unnamed_addr = true;
                    self.advance();
                }
                other if is_attr_keyword(other) => {
                    attrs.push(self.parse_attribute()?);
                }
                _ => break,
            }
        }
        let ret_ty = self.parse_type()?;
        let (name, _) = self.expect_global()?;
        let symbol = self.module.intern(&name);
        let mut function = Function::new(symbol, 0);
        function.linkage = linkage;
        function.visibility = visibility;
        function.dso_local = dso_local;
        function.unnamed_addr = unnamed_addr;
        function.is_declaration = is_declaration;
        function.attrs = attrs;
        self.expect_punct('(')?;
        let mut param_types = Vec::new();
        let mut varargs = false;
        if !self.at_punct(')') {
            loop {
                if self.eat_ellipsis() {
                    varargs = true;
                    break;
                }
                let param_attrs = self.parse_param_attrs()?;
                let ty = self.parse_type()?;
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
        self.parse_trailing_attrs(&mut function.attrs, !is_declaration)?;
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
                        match text.as_str() {
                            "section" | "gc" | "prefix" | "prologue" | "personality" => {
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
                            "comdat" => {
                                let mut text = String::from("comdat");
                                if self.eat_punct('(') {
                                    let (name, _) = self.expect_global()?;
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
                self.expect_punct('=')?;
                let value = self.expect_string()?;
                Ok(Attribute(format!(
                    "{}={}",
                    quote_bytes(&key),
                    quote_bytes(value.as_bytes())
                )))
            }
            _ => Err(self.error("expected an attribute")),
        }
    }

    fn parse_attribute(&mut self) -> Result<Attribute> {
        let name = self.expect_ident()?;
        let mut text = name.clone();
        if self.at_punct('(') {
            self.consume_balanced_text(&mut text)?;
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
            let token = self.tokens[self.pos].clone();
            self.pos += 1;
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

    fn local_symbol(&mut self, name: Option<String>) -> Result<Symbol> {
        match name {
            Some(name) => {
                if let Ok(value) = name.parse::<u32>() {
                    self.next_local = self.next_local.max(value.saturating_add(1));
                }
                Ok(self.module.intern(&name))
            }
            None => {
                let value = self.next_local;
                self.next_local += 1;
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
            if let Some(name) = result_name {
                let ty =
                    result_ty.ok_or_else(|| self.error("instruction does not produce a value"))?;
                let symbol = self.local_symbol(Some(name))?;
                let local = function.add_local(Some(symbol), ty, inst_id);
                self.local_names.insert(symbol, local);
                function.insts[inst_id].result = Some(local);
            }
            if self.at_eol() {
                self.advance();
            }
        }
    }

    fn parse_label_name(&mut self) -> Result<Symbol> {
        let name = match self.peek_kind().clone() {
            TokenKind::Ident(name) | TokenKind::Local(name) => name,
            TokenKind::Int(value) => value.to_string(),
            TokenKind::Str(bytes) => {
                String::from_utf8(bytes).map_err(|_| self.error("label is not valid UTF-8"))?
            }
            _ => return Err(self.error("expected a basic block label")),
        };
        self.advance();
        self.expect_punct(':')?;
        if let Ok(value) = name.parse::<u32>() {
            self.next_block = self.next_block.max(value.saturating_add(1));
        }
        Ok(self.module.intern(&name))
    }

    fn auto_block_name(&mut self) -> Symbol {
        let value = self.next_block;
        self.next_block += 1;
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
        let base = match self.peek_kind().clone() {
            TokenKind::Ident(text) => {
                self.advance();
                match text.as_str() {
                    "void" => self.module.types.void(),
                    "label" => self.module.types.intern(Type::Label),
                    "token" => self.module.types.intern(Type::Token),
                    "metadata" => self.module.types.intern(Type::Metadata),
                    "opaque" => self.module.types.intern(Type::Opaque),
                    "ptr" => {
                        if self.eat_ident("addrspace") {
                            self.expect_punct('(')?;
                            let _ = self.expect_int()?;
                            self.expect_punct(')')?;
                        }
                        self.module.types.pointer()
                    }
                    "half" => self.module.types.intern(Type::Float(FloatKind::Half)),
                    "bfloat" => self.module.types.intern(Type::Float(FloatKind::BFloat)),
                    "float" => self.module.types.intern(Type::Float(FloatKind::Float)),
                    "double" => self.module.types.intern(Type::Float(FloatKind::Double)),
                    "fp128" => self.module.types.intern(Type::Float(FloatKind::Fp128)),
                    "x86_fp80" => self.module.types.intern(Type::Float(FloatKind::X86Fp80)),
                    "ppc_fp128" => self.module.types.intern(Type::Float(FloatKind::PpcFp128)),
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
                }
            }
            TokenKind::Local(name) => {
                self.advance();
                self.module.types.named(&name)
            }
            TokenKind::Punct('[') => {
                self.advance();
                let len = self.expect_int()? as u64;
                self.expect_ident_is("x")?;
                let elem = self.parse_type()?;
                self.expect_punct(']')?;
                self.module.types.array(elem, len)
            }
            TokenKind::Punct('<') => {
                self.advance();
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
                self.advance();
                let fields = self.parse_struct_fields('}')?;
                self.module.types.intern(Type::Struct {
                    fields,
                    packed: false,
                    name: None,
                })
            }
            other => {
                return Err(self.error(format!("expected a type, found {other:?}")));
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
        match self.peek_kind().clone() {
            TokenKind::Int(value) => {
                self.advance();
                let bits =
                    self.module.types.get(ty).int_bits().ok_or_else(|| {
                        self.error("integer constant used with a non-integer type")
                    })?;
                let value = mask_int(value, bits);
                Ok(self.module.const_int(ty, value))
            }
            TokenKind::Float(text) => {
                self.advance();
                let kind = match self.module.types.get(ty) {
                    Type::Float(kind) => *kind,
                    _ => return Err(self.error("floating constant used with a non-float type")),
                };
                let bits = parse_float_bits(&text, kind)
                    .ok_or_else(|| self.error(format!("invalid floating literal `{text}`")))?;
                Ok(self.module.const_float_bits(ty, bits))
            }
            TokenKind::CStr(bytes) => {
                self.advance();
                Ok(self
                    .module
                    .add_constant(Constant::new(ty, ConstKind::String(bytes))))
            }
            TokenKind::Global(name) => {
                self.advance();
                let symbol = self.module.intern(&name);
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
                self.advance();
                let kind = match text.as_str() {
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
                self.advance();
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
                self.advance();
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
            TokenKind::Punct('{') => {
                self.advance();
                self.parse_struct_constant(ty)
            }
            TokenKind::Local(name) => {
                Err(self.error(format!("local value %{name} is not a constant")))
            }
            other => Err(self.error(format!("expected a constant, found {other:?}"))),
        }
    }

    fn parse_struct_constant(&mut self, ty: TypeId) -> Result<ConstId> {
        let (fields, _) = self
            .module
            .types
            .struct_body(ty)
            .ok_or_else(|| self.error("struct constant used with a non-struct type"))?;
        let fields = fields.to_vec();
        let mut elems = Vec::new();
        if !self.eat_punct('}') {
            let mut index = 0;
            loop {
                let _ = self.parse_type()?;
                let field_ty = fields.get(index).copied().unwrap_or(ty);
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

    fn parse_value(&mut self, function: &Function, ty: TypeId) -> Result<Operand> {
        match self.peek_kind().clone() {
            TokenKind::Local(name) => {
                self.advance();
                let symbol = self.module.intern(&name);
                let local = *self
                    .local_names
                    .get(&symbol)
                    .ok_or_else(|| self.error(format!("use of undefined local value %{name}")))?;
                Ok(Operand::local(function.local_type(local), local))
            }
            TokenKind::Global(name) => {
                self.advance();
                let symbol = self.module.intern(&name);
                if let Some(id) = self.module.function_named(symbol) {
                    Ok(Operand::function(ty, id))
                } else {
                    let id = self.module.global_or_insert(symbol);
                    Ok(Operand::global(ty, id))
                }
            }
            _ => {
                let id = self.parse_constant(ty)?;
                Ok(Operand::constant(ty, id))
            }
        }
    }

    fn parse_phi_block(&mut self, function: &mut Function) -> Result<BlockId> {
        let name = match self.peek_kind().clone() {
            TokenKind::Local(name) => name,
            TokenKind::Int(value) => value.to_string(),
            _ => return Err(self.error("expected a phi predecessor block")),
        };
        self.advance();
        let symbol = self.module.intern(&name);
        Ok(function.block_or_insert(symbol))
    }

    fn parse_block_ref(&mut self, function: &mut Function) -> Result<BlockId> {
        self.expect_ident_is("label")?;
        let name = match self.peek_kind().clone() {
            TokenKind::Local(name) => name,
            TokenKind::Int(value) => value.to_string(),
            _ => return Err(self.error("expected a basic block reference")),
        };
        self.advance();
        let symbol = self.module.intern(&name);
        Ok(function.block_or_insert(symbol))
    }

    fn parse_callee(&mut self, function: &Function) -> Result<Operand> {
        match self.peek_kind().clone() {
            TokenKind::Global(name) => {
                self.advance();
                let symbol = self.module.intern(&name);
                if let Some(id) = self.module.function_named(symbol) {
                    Ok(Operand::function(self.module.functions[id].ty, id))
                } else if let Some(id) = self.module.global_named(symbol) {
                    Ok(Operand::global(self.module.globals[id].ty, id))
                } else {
                    let id = self.module.function_or_insert(symbol);
                    Ok(Operand::function(INVALID_TYPE, id))
                }
            }
            TokenKind::Local(name) => {
                self.advance();
                let symbol = self.module.intern(&name);
                let local = *self
                    .local_names
                    .get(&symbol)
                    .ok_or_else(|| self.error(format!("use of undefined local value %{name}")))?;
                Ok(Operand::local(function.local_type(local), local))
            }
            _ => {
                let ty = self.parse_type()?;
                self.parse_value(function, ty)
            }
        }
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
                        TokenKind::Ident(text) if text == "nsw" => {
                            flags.nsw = true;
                            self.advance();
                        }
                        TokenKind::Ident(text) if text == "nuw" => {
                            flags.nuw = true;
                            self.advance();
                        }
                        TokenKind::Ident(text) if text == "exact" => {
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
        match mnemonic.as_str() {
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
                let i1 = self.module.types.int(1);
                Ok((InstKind::ICmp { pred, lhs, rhs }, Some(i1)))
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
                let i1 = self.module.types.int(1);
                Ok((
                    InstKind::FCmp {
                        pred,
                        flags,
                        lhs,
                        rhs,
                    },
                    Some(i1),
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
                let volatile = self.eat_ident("volatile");
                let ty = self.parse_type()?;
                self.expect_punct(',')?;
                let ptr_ty = self.parse_type()?;
                let ptr = self.parse_value(function, ptr_ty)?;
                let align = self.parse_optional_align()?;
                Ok((
                    InstKind::Load {
                        ty,
                        ptr,
                        align,
                        volatile,
                    },
                    Some(ty),
                ))
            }
            "store" => {
                self.advance();
                let volatile = self.eat_ident("volatile");
                let ty = self.parse_type()?;
                let value = self.parse_value(function, ty)?;
                self.expect_punct(',')?;
                let ptr_ty = self.parse_type()?;
                let ptr = self.parse_value(function, ptr_ty)?;
                let align = self.parse_optional_align()?;
                Ok((
                    InstKind::Store {
                        value,
                        ptr,
                        align,
                        volatile,
                    },
                    None,
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
            "tail" | "musttail" | "notail" => {
                let tail = match mnemonic.as_str() {
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
                    let value = self.parse_value(function, ty)?;
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
                if !self.eat_punct(']') {
                    loop {
                        let case_ty = self.parse_type()?;
                        let case_value = self.parse_constant(case_ty)?;
                        self.expect_punct(',')?;
                        let block = self.parse_block_ref(function)?;
                        cases.push((case_value, block));
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

    fn parse_call(
        &mut self,
        function: &mut Function,
        tail: TailKind,
    ) -> Result<(InstKind, Option<TypeId>)> {
        self.expect_ident_is("call")?;
        let fastmath = self.parse_fastmath()?;
        let mut attrs = Vec::new();
        if let TokenKind::Ident(text) = self.peek_kind() {
            if text == "cc" {
                self.advance();
                let value = self.expect_int()?;
                attrs.push(Attribute(format!("cc {value}")));
            } else if matches!(
                text.as_str(),
                "fastcc" | "coldcc" | "swiftcc" | "tailcc" | "preserve_mostcc" | "preserve_allcc"
            ) {
                attrs.push(Attribute(text.clone()));
                self.advance();
            }
        }
        let ty = self.parse_type()?;
        let (ret_ty, _explicit_params) = match self.module.types.get(ty) {
            Type::Function { ret, params, .. } => (*ret, Some(params.clone())),
            _ => (ty, None),
        };
        let callee = self.parse_callee(function)?;
        self.expect_punct('(')?;
        let mut args = Vec::new();
        if !self.at_punct(')') {
            loop {
                let arg_ty = self.parse_type()?;
                args.push(self.parse_value(function, arg_ty)?);
                if !self.eat_punct(',') {
                    break;
                }
            }
        }
        self.expect_punct(')')?;
        self.parse_call_attrs(&mut attrs)?;
        let result_ty = if self.module.types.get(ret_ty).is_void() {
            None
        } else {
            Some(ret_ty)
        };
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
        Ok((
            InstKind::Call {
                tail,
                ret_ty,
                fn_ty,
                callee,
                args,
                fastmath,
                attrs,
            },
            result_ty,
        ))
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

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_kind_at(&self, offset: usize) -> &TokenKind {
        self.tokens
            .get(self.pos + offset)
            .map(|token| &token.kind)
            .unwrap_or(&self.tokens[self.tokens.len() - 1].kind)
    }

    fn advance(&mut self) {
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
    }

    fn take(&mut self) -> Token {
        let token = self.tokens[self.pos].clone();
        self.advance();
        token
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
        matches!(self.peek_kind(), TokenKind::Ident(found) if found == text)
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
        match self.peek_kind().clone() {
            TokenKind::Ident(text) => {
                self.advance();
                Ok(text)
            }
            other => Err(self.error(format!("expected an identifier, found {other:?}"))),
        }
    }

    fn expect_ident_is(&mut self, expected: &str) -> Result<()> {
        let text = self.expect_ident()?;
        if text == expected {
            Ok(())
        } else {
            Err(self.error(format!("expected `{expected}`, found `{text}`")))
        }
    }

    fn expect_local(&mut self) -> Result<(String, Span)> {
        match self.peek_kind().clone() {
            TokenKind::Local(name) => {
                let span = self.peek().span;
                self.advance();
                Ok((name, span))
            }
            other => Err(self.error(format!("expected a local name, found {other:?}"))),
        }
    }

    fn expect_global(&mut self) -> Result<(String, Span)> {
        match self.peek_kind().clone() {
            TokenKind::Global(name) => {
                let span = self.peek().span;
                self.advance();
                Ok((name, span))
            }
            other => Err(self.error(format!("expected a global name, found {other:?}"))),
        }
    }

    fn expect_int(&mut self) -> Result<u128> {
        match self.peek_kind().clone() {
            TokenKind::Int(value) => {
                self.advance();
                Ok(value)
            }
            other => Err(self.error(format!("expected an integer, found {other:?}"))),
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

    fn skip_balanced(&mut self, close: char) -> Result<()> {
        let mut depth = 1usize;
        while depth > 0 {
            if self.at_eof() {
                return Err(self.error("unterminated parenthesized group"));
            }
            let token = self.take();
            match token.kind {
                TokenKind::Punct('(') => depth += 1,
                TokenKind::Punct(found) if found == close => depth -= 1,
                _ => {}
            }
        }
        Ok(())
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
        assert_eq!(tokens[0].kind, TokenKind::Ident("attributes".to_string()));
        assert_eq!(tokens[1].kind, TokenKind::AttrGroup(0));
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

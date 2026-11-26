//! Attribute-list encoding for the LLVM bitcode subset.
//!
//! The IR keeps attributes as their textual spelling.  This module translates
//! the spellings we support to and from the PARAMATTR records used by LLVM.

use std::collections::HashMap;

use llvm_rust_ir::names::{quote_bytes, unescape_bytes};
use llvm_rust_ir::{Attribute, Module, TypeId, TypeTable};
use llvm_rust_support::{Error, Result};

use crate::bitstream::Record;

pub(crate) const PARAMATTR_CODE_ENTRY: u64 = 2;
pub(crate) const PARAMATTR_GRP_CODE_ENTRY: u64 = 3;

const FUNCTION_INDEX: u32 = u32::MAX;

/// Expands `#N` references and returns the concrete attributes in `attrs`.
///
/// LLVM applies numbered groups after inline attributes, so a group wins over
/// an earlier inline value for the same string key.
pub(crate) fn expand_attributes(module: &Module, attrs: &[Attribute]) -> Result<Vec<Attribute>> {
    let mut expanded = Vec::new();
    let mut groups = Vec::new();
    for attr in attrs {
        if attr.0.trim_start().starts_with('#') {
            groups.push(attr);
        } else {
            expanded.push(Attribute(attr.0.trim().to_string()));
        }
    }
    for attr in groups {
        expand_attribute(module, attr, 0, &mut expanded)?;
    }
    Ok(expanded)
}

fn expand_attribute(
    module: &Module,
    attr: &Attribute,
    depth: usize,
    out: &mut Vec<Attribute>,
) -> Result<()> {
    let text = attr.0.trim();
    let Some(number) = text.strip_prefix('#') else {
        out.push(Attribute(text.to_string()));
        return Ok(());
    };
    if depth > 8 {
        return Err(Error::bitcode("attribute group reference cycle"));
    }
    let number = number
        .parse::<u32>()
        .map_err(|_| Error::bitcode(format!("invalid attribute group `{text}`")))?;
    let group = module
        .attr_groups
        .iter()
        .find(|(id, _)| *id == number)
        .ok_or_else(|| Error::bitcode(format!("unknown attribute group `{text}`")))?;
    for part in split_attributes(&group.1)? {
        expand_attribute(module, &Attribute(part), depth + 1, out)?;
    }
    Ok(())
}

/// Splits a textual attribute group into individual attribute spellings.
fn split_attributes(text: &str) -> Result<Vec<String>> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut depth = 0usize;
    for ch in text.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                current.push(ch);
            }
            '(' if !quoted => {
                depth += 1;
                current.push(ch);
            }
            ')' if !quoted => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| Error::bitcode("unbalanced attribute parentheses"))?;
                current.push(ch);
            }
            ch if ch.is_whitespace() && !quoted && depth == 0 => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if quoted || depth != 0 {
        return Err(Error::bitcode("unterminated attribute group"));
    }
    if !current.is_empty() {
        parts.push(current);
    }

    let mut joined = Vec::with_capacity(parts.len());
    let mut index = 0;
    while index < parts.len() {
        if parts[index] == "align" && index + 1 < parts.len() {
            joined.push(format!("align {}", parts[index + 1]));
            index += 2;
        } else if let Some(value) = parts[index].strip_prefix("align=") {
            joined.push(format!("align {value}"));
            index += 1;
        } else {
            joined.push(parts[index].clone());
            index += 1;
        }
    }
    Ok(joined)
}

/// Identity used when LLVM merges two attribute groups for the same slot.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum AttributeKey {
    Enum(u64),
    Int(u64),
    Type(u64),
    String(Vec<u8>),
    Memory,
    Raw(String),
}

fn attribute_key(attr: &Attribute) -> AttributeKey {
    let text = attr.0.trim();
    if text.starts_with('"') {
        if let Ok(Some((kind, _))) = parse_string_attribute(text) {
            return AttributeKey::String(kind);
        }
        return AttributeKey::Raw(text.to_string());
    }
    if text.starts_with("memory(") {
        return AttributeKey::Memory;
    }
    if let Some(code) = int_attribute_code(text) {
        return AttributeKey::Int(code);
    }
    if let Some(code) = type_attribute_code(text) {
        return AttributeKey::Type(code);
    }
    if let Some(code) = enum_attribute_code(text) {
        return AttributeKey::Enum(code);
    }
    AttributeKey::Raw(text.to_string())
}

fn insert_memory_attribute(attrs: &mut Vec<Attribute>, attr: Attribute) {
    let position = attrs
        .iter()
        .position(|existing| !matches!(attribute_key(existing), AttributeKey::Enum(_)))
        .unwrap_or(attrs.len());
    attrs.insert(position, attr);
}

fn int_attribute_code(text: &str) -> Option<u64> {
    if text.starts_with("align ") || text.starts_with("align(") || text.starts_with("align=") {
        Some(1)
    } else if text.starts_with("alignstack ")
        || text.starts_with("alignstack(")
        || text.starts_with("alignstack=")
    {
        Some(25)
    } else if text.starts_with("dereferenceable(") {
        Some(41)
    } else if text.starts_with("dereferenceable_or_null(") {
        Some(42)
    } else {
        None
    }
}

fn type_attribute_code(text: &str) -> Option<u64> {
    for (name, code) in [
        ("byval", 3),
        ("sret", 29),
        ("inalloca", 38),
        ("preallocated", 65),
        ("byref", 69),
        ("elementtype", 77),
    ] {
        if text == name || text.starts_with(&format!("{name}(")) {
            return Some(code);
        }
    }
    None
}

/// Merges `incoming` into `existing` with LLVM's last-writer-wins rule for
/// attributes of the same kind or string key.
pub(crate) fn merge_attributes(existing: &mut Vec<Attribute>, incoming: &[Attribute]) {
    let mut positions: HashMap<AttributeKey, usize> = existing
        .iter()
        .enumerate()
        .map(|(index, attr)| (attribute_key(attr), index))
        .collect();
    for attr in incoming {
        let key = attribute_key(attr);
        if let Some(&position) = positions.get(&key) {
            existing[position] = attr.clone();
        } else {
            positions.insert(key, existing.len());
            existing.push(attr.clone());
        }
    }
}

/// Collapses duplicate attributes in one decoded group, keeping the last value
/// for each kind or string key.
pub(crate) fn deduplicate_attributes(attrs: &mut Vec<Attribute>) {
    let mut positions: HashMap<AttributeKey, usize> = HashMap::new();
    let mut write = 0;
    for read in 0..attrs.len() {
        let key = attribute_key(&attrs[read]);
        if let Some(&position) = positions.get(&key) {
            attrs[position] = attrs[read].clone();
        } else {
            positions.insert(key, write);
            if write != read {
                attrs.swap(write, read);
            }
            write += 1;
        }
    }
    attrs.truncate(write);
}

/// Encodes one attribute as the operands of a PARAMATTR group entry.
pub(crate) fn encode_attribute(attr: &Attribute) -> Result<Vec<u64>> {
    let text = attr.0.trim();
    if let Some((kind, value)) = parse_string_attribute(text)? {
        let mut operands = vec![if value.is_some() { 4 } else { 3 }];
        operands.extend(kind.into_iter().map(u64::from));
        operands.push(0);
        if let Some(value) = value {
            operands.extend(value.into_iter().map(u64::from));
            operands.push(0);
        }
        return Ok(operands);
    }

    if let Some(value) = text.strip_prefix("align ").or_else(|| {
        text.strip_prefix("align(")
            .and_then(|value| value.strip_suffix(')'))
    }) {
        return Ok(vec![1, 1, parse_u64(value.trim(), "align")?]);
    }
    if let Some(value) = text
        .strip_prefix("alignstack ")
        .or_else(|| {
            text.strip_prefix("alignstack(")
                .and_then(|value| value.strip_suffix(')'))
        })
        .or_else(|| text.strip_prefix("alignstack="))
    {
        return Ok(vec![1, 25, parse_u64(value.trim(), "alignstack")?]);
    }
    if let Some(value) = text
        .strip_prefix("dereferenceable(")
        .and_then(|value| value.strip_suffix(')'))
    {
        return Ok(vec![1, 41, parse_u64(value, "dereferenceable")?]);
    }
    if let Some(value) = text
        .strip_prefix("dereferenceable_or_null(")
        .and_then(|value| value.strip_suffix(')'))
    {
        return Ok(vec![1, 42, parse_u64(value, "dereferenceable_or_null")?]);
    }
    if text.starts_with("memory(") {
        let effects = parse_memory_attribute(text)
            .ok_or_else(|| Error::bitcode(format!("unsupported memory attribute `{text}`")))?;
        return Ok(vec![1, 86, u64::from(effects.0)]);
    }

    if let Some(code) = enum_attribute_code(text) {
        return Ok(vec![0, code]);
    }
    Err(Error::bitcode(format!(
        "unsupported attribute in bitcode writer: `{text}`"
    )))
}

fn parse_string_attribute(text: &str) -> Result<Option<(Vec<u8>, Option<Vec<u8>>)>> {
    if !text.starts_with('"') {
        return Ok(None);
    }
    let end = text[1..]
        .find('"')
        .map(|offset| offset + 1)
        .ok_or_else(|| Error::bitcode("unterminated string attribute"))?;
    let kind = unescape_bytes(&text[1..end])
        .ok_or_else(|| Error::bitcode("invalid string attribute key escape"))?;
    let rest = text[end + 1..].trim();
    if rest.is_empty() {
        return Ok(Some((kind, None)));
    }
    let rest = rest
        .strip_prefix('=')
        .ok_or_else(|| Error::bitcode("malformed string attribute"))?
        .trim();
    if !rest.starts_with('"') || !rest.ends_with('"') || rest.len() < 2 {
        return Err(Error::bitcode("malformed string attribute value"));
    }
    let value = unescape_bytes(&rest[1..rest.len() - 1])
        .ok_or_else(|| Error::bitcode("invalid string attribute value escape"))?;
    Ok(Some((kind, Some(value))))
}

fn parse_u64(text: &str, name: &str) -> Result<u64> {
    text.parse::<u64>()
        .map_err(|_| Error::bitcode(format!("invalid `{name}` value `{text}`")))
}

const ENUM_ATTRIBUTES: &[(&str, u64)] = &[
    ("alwaysinline", 2),
    ("inlinehint", 4),
    ("inreg", 5),
    ("minsize", 6),
    ("naked", 7),
    ("nest", 8),
    ("noalias", 9),
    ("nobuiltin", 10),
    ("nocapture", 11),
    ("noduplicate", 12),
    ("noimplicitfloat", 13),
    ("noinline", 14),
    ("nonlazybind", 15),
    ("noredzone", 16),
    ("noreturn", 17),
    ("nounwind", 18),
    ("optsize", 19),
    ("memory(none)", 20),
    ("memory(read)", 21),
    ("returned", 22),
    ("returns_twice", 23),
    ("signext", 24),
    ("ssp", 26),
    ("sspreq", 27),
    ("sspstrong", 28),
    ("sanitize_address", 30),
    ("sanitize_thread", 31),
    ("sanitize_memory", 32),
    ("uwtable", 33),
    ("zeroext", 34),
    ("builtin", 35),
    ("cold", 36),
    ("optnone", 37),
    ("nonnull", 39),
    ("jumptable", 40),
    ("convergent", 43),
    ("safestack", 44),
    ("argmemonly", 45),
    ("swiftself", 46),
    ("swifterror", 47),
    ("norecurse", 48),
    ("inaccessiblememonly", 49),
    ("inaccessiblemem_or_argmemonly", 50),
    ("memory(write)", 52),
    ("speculatable", 53),
    ("strictfp", 54),
    ("sanitize_hwaddress", 55),
    ("nocf_check", 56),
    ("optforfuzzing", 57),
    ("shadowcallstack", 58),
    ("speculative_load_hardening", 59),
    ("immarg", 60),
    ("willreturn", 61),
    ("nofree", 62),
    ("nosync", 63),
    ("sanitize_memtag", 64),
    ("nomerge", 66),
    ("null_pointer_is_valid", 67),
    ("noundef", 68),
    ("mustprogress", 70),
    ("nocallback", 71),
    ("hot", 72),
    ("noprofile", 73),
    ("swiftasync", 75),
    ("nosanitize_coverage", 76),
    ("disable_sanitizer_instrumentation", 78),
    ("nosanitize_bounds", 79),
    ("presplitcoroutine", 83),
    ("fnretthunk_extern", 84),
    ("skipprofile", 85),
    ("optdebug", 88),
    ("coro_only_destroy_when_complete", 90),
    ("dead_on_unwind", 91),
    ("sanitize_numerical_stability", 93),
    ("hybrid_patchable", 95),
    ("sanitize_realtime", 96),
    ("sanitize_realtime_blocking", 97),
    ("coro_elide_safe", 98),
    ("no_ext", 99),
    ("no_divergence_source", 100),
    ("sanitize_type", 101),
    ("dead_on_return", 103),
    ("sanitize_alloc_token", 104),
    ("no_create_undef_or_poison", 105),
    ("denormal_fpenv", 106),
    ("nooutline", 107),
    ("flatten", 108),
    ("noipa", 109),
];

fn enum_attribute_code(text: &str) -> Option<u64> {
    match text {
        "readnone" => return Some(20),
        "readonly" => return Some(21),
        "writeonly" => return Some(52),
        _ => {}
    }
    ENUM_ATTRIBUTES
        .iter()
        .find_map(|(name, code)| (*name == text).then_some(*code))
}

/// Memory effects as a 2-bit mod/ref mask for each LLVM 19 location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MemoryEffects(u8);

const MEM_ARG: u8 = 0;
const MEM_INACCESSIBLE: u8 = 2;
const MEM_OTHER: u8 = 4;

impl MemoryEffects {
    const fn none() -> Self {
        Self(0)
    }

    const fn unknown() -> Self {
        Self(0x3F)
    }

    fn all(modref: u8) -> Self {
        let mut effects = Self::none();
        for location in [MEM_ARG, MEM_INACCESSIBLE, MEM_OTHER] {
            effects.set(location, modref);
        }
        effects
    }

    fn only(location: u8) -> Self {
        let mut effects = Self::none();
        effects.set(location, 3);
        effects
    }

    fn get(self, location: u8) -> u8 {
        (self.0 >> location) & 3
    }

    fn set(&mut self, location: u8, modref: u8) {
        self.0 = (self.0 & !(3 << location)) | ((modref & 3) << location);
    }

    fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    fn union_modref(self) -> u8 {
        [MEM_ARG, MEM_INACCESSIBLE, MEM_OTHER]
            .into_iter()
            .fold(0, |acc, location| acc | self.get(location))
    }

    fn from_int(value: u64, is_aarch64: bool) -> Result<Self> {
        let version = value >> 56;
        let payload = value & 0x00FF_FFFF_FFFF_FFFF;
        match version {
            0 => {
                if payload & !0x3F != 0 {
                    return Err(Error::bitcode(
                        "unsupported unversioned memory attribute layout",
                    ));
                }
                Ok(Self(payload as u8))
            }
            1 => {
                if payload & !0xFF != 0 {
                    return Err(Error::bitcode("unsupported version 1 memory layout"));
                }
                let mut effects = Self::none();
                effects.set(MEM_ARG, (payload & 3) as u8);
                effects.set(MEM_INACCESSIBLE, ((payload >> 2) & 3) as u8);
                effects.set(MEM_OTHER, (((payload >> 6) | (payload >> 4)) & 3) as u8);
                Ok(effects)
            }
            2 => {
                if payload & !0xFFF != 0 {
                    return Err(Error::bitcode("unsupported version 2 memory layout"));
                }
                let inaccessible = ((payload >> 2) & 3) as u8;
                let other = (((payload >> 6) | (payload >> 4)) & 3) as u8;
                let reference = if is_aarch64 { inaccessible } else { other };
                let target0 = (payload >> 8) & 3;
                let target1 = (payload >> 10) & 3;
                if (target0 != 0 && target0 != u64::from(reference))
                    || (target1 != 0 && target1 != u64::from(reference))
                {
                    return Err(Error::bitcode(
                        "target memory effects cannot be represented",
                    ));
                }
                let mut effects = Self::none();
                effects.set(MEM_ARG, (payload & 3) as u8);
                effects.set(MEM_INACCESSIBLE, inaccessible);
                effects.set(MEM_OTHER, other);
                Ok(effects)
            }
            _ => Err(Error::bitcode(format!(
                "unsupported memory attribute version {version}"
            ))),
        }
    }

    fn text(self) -> String {
        let other = self.get(MEM_OTHER);
        let union = self.union_modref();
        let mut parts = Vec::new();
        if other != 0 || union == other {
            parts.push(modref_name(other).to_string());
        }
        for (location, name) in [(MEM_ARG, "argmem"), (MEM_INACCESSIBLE, "inaccessiblemem")] {
            let modref = self.get(location);
            if modref != other {
                parts.push(format!("{name}: {}", modref_name(modref)));
            }
        }
        format!("memory({})", parts.join(", "))
    }
}

fn modref_name(modref: u8) -> &'static str {
    match modref {
        0 => "none",
        1 => "read",
        2 => "write",
        _ => "readwrite",
    }
}

fn legacy_memory_effect(code: u64) -> Option<MemoryEffects> {
    match code {
        20 => Some(MemoryEffects::none()),
        21 => Some(MemoryEffects::all(1)),
        52 => Some(MemoryEffects::all(2)),
        45 => Some(MemoryEffects::only(MEM_ARG)),
        49 => Some(MemoryEffects::only(MEM_INACCESSIBLE)),
        50 => {
            let mut effects = MemoryEffects::only(MEM_ARG);
            effects.set(MEM_INACCESSIBLE, 3);
            Some(effects)
        }
        _ => None,
    }
}

fn legacy_memory_effect_name(text: &str) -> Option<MemoryEffects> {
    let code = match text {
        "readnone" => 20,
        "readonly" => 21,
        "writeonly" => 52,
        "argmemonly" => 45,
        "inaccessiblememonly" => 49,
        "inaccessiblemem_or_argmemonly" => 50,
        _ => return None,
    };
    legacy_memory_effect(code)
}

fn parse_modref(text: &str) -> Option<u8> {
    match text {
        "none" => Some(0),
        "read" => Some(1),
        "write" => Some(2),
        "readwrite" => Some(3),
        _ => None,
    }
}

struct ParsedMemory {
    effects: MemoryEffects,
    errno: Option<u8>,
    default_modref: Option<u8>,
    target0: u8,
    target1: u8,
    target_explicit: bool,
}

fn parse_memory_attribute_parts(text: &str) -> Option<ParsedMemory> {
    let inner = text.strip_prefix("memory(")?.strip_suffix(')')?;
    let mut parsed = ParsedMemory {
        effects: MemoryEffects::none(),
        errno: None,
        default_modref: None,
        target0: 0,
        target1: 0,
        target_explicit: false,
    };
    let mut seen_location = false;
    for part in inner.split(',') {
        let part = part.trim();
        if let Some((location, modref)) = part.split_once(':') {
            let modref = parse_modref(modref.trim())?;
            match location.trim() {
                "argmem" => parsed.effects.set(MEM_ARG, modref),
                "inaccessiblemem" => parsed.effects.set(MEM_INACCESSIBLE, modref),
                "errnomem" => parsed.errno = Some(modref),
                "target_mem0" => {
                    parsed.target0 = modref;
                    parsed.target_explicit = true;
                }
                "target_mem1" => {
                    parsed.target1 = modref;
                    parsed.target_explicit = true;
                }
                "target_mem" => {
                    parsed.target0 = modref;
                    parsed.target1 = modref;
                    parsed.target_explicit = true;
                }
                _ => return None,
            }
            seen_location = true;
        } else {
            if seen_location {
                return None;
            }
            let modref = parse_modref(part)?;
            parsed.effects = MemoryEffects::all(modref);
            parsed.default_modref = Some(modref);
            parsed.target0 = modref;
            parsed.target1 = modref;
        }
    }
    Some(parsed)
}

fn parse_memory_attribute(text: &str) -> Option<MemoryEffects> {
    let parsed = parse_memory_attribute_parts(text)?;
    if parsed.target_explicit {
        return None;
    }
    if let Some(errno) = parsed.errno {
        if errno != parsed.effects.get(MEM_OTHER) {
            return None;
        }
    }
    Some(parsed.effects)
}

fn is_aarch64_triple(triple: Option<&str>) -> bool {
    triple.is_some_and(|triple| {
        triple
            .split('-')
            .next()
            .is_some_and(|arch| arch.starts_with("aarch64") || arch.starts_with("arm64"))
    })
}

fn resolve_function_attributes(
    attrs: Vec<Attribute>,
    target_triple: Option<&str>,
) -> Result<Vec<Attribute>> {
    let mut resolved = Vec::new();
    let mut legacy_memory = MemoryEffects::unknown();
    let mut legacy_memory_seen = false;
    let mut modern_memory = None;
    let is_aarch64 = is_aarch64_triple(target_triple);
    for attr in attrs {
        let text = attr.0.trim();
        if text.starts_with("memory(") {
            let parsed = parse_memory_attribute_parts(text)
                .ok_or_else(|| Error::bitcode(format!("unsupported memory attribute `{text}`")))?;
            if let Some(errno) = parsed.errno {
                if errno != parsed.effects.get(MEM_OTHER) {
                    return Err(Error::bitcode(format!(
                        "errno memory effect in `{text}` cannot be represented"
                    )));
                }
            }
            let inaccessible = parsed.effects.get(MEM_INACCESSIBLE);
            if parsed.target_explicit {
                if !is_aarch64 {
                    return Err(Error::bitcode(format!(
                        "target memory effects in `{text}` require an AArch64 target"
                    )));
                }
                if parsed.target0 != inaccessible || parsed.target1 != inaccessible {
                    return Err(Error::bitcode(format!(
                        "target memory effects in `{text}` cannot survive LLVM 19 encoding"
                    )));
                }
            } else if is_aarch64 {
                if let Some(default) = parsed.default_modref {
                    if default != 0 && default != inaccessible {
                        return Err(Error::bitcode(format!(
                            "target memory effects in `{text}` cannot survive LLVM 19 encoding"
                        )));
                    }
                }
            }
            modern_memory = Some(parsed.effects);
        } else if let Some(effect) = legacy_memory_effect_name(text) {
            legacy_memory = legacy_memory.intersect(effect);
            legacy_memory_seen = true;
        } else {
            resolved.push(Attribute(text.to_string()));
        }
    }
    if legacy_memory_seen {
        modern_memory = Some(legacy_memory);
    }
    if let Some(effects) = modern_memory {
        insert_memory_attribute(&mut resolved, Attribute(effects.text()));
    }
    Ok(resolved)
}

/// Expands function or call-site attributes while keeping inline and numbered
/// groups as separate memory-effect sources. Numbered groups apply after inline
/// attributes, and the last resolved memory attribute wins.
pub(crate) fn expand_function_attributes(
    module: &Module,
    attrs: &[Attribute],
) -> Result<Vec<Attribute>> {
    let mut expanded = Vec::new();
    let inline: Vec<Attribute> = attrs
        .iter()
        .filter(|attr| !attr.0.trim_start().starts_with('#'))
        .map(|attr| Attribute(attr.0.trim().to_string()))
        .collect();
    if !inline.is_empty() {
        let resolved = resolve_function_attributes(inline, module.target_triple.as_deref())?;
        merge_attributes(&mut expanded, &resolved);
    }
    for attr in attrs {
        if !attr.0.trim_start().starts_with('#') {
            continue;
        }
        let mut group_attrs = Vec::new();
        expand_attribute(module, attr, 0, &mut group_attrs)?;
        let resolved = resolve_function_attributes(group_attrs, module.target_triple.as_deref())?;
        merge_attributes(&mut expanded, &resolved);
    }
    Ok(expanded)
}

/// Decodes one PARAMATTR group entry.
pub(crate) fn decode_attribute_group(
    record: &Record,
    types: &TypeTable,
    type_ids: &[TypeId],
    target_triple: Option<&str>,
) -> Result<(u64, u32, Vec<Attribute>)> {
    if record.operands.len() < 3 {
        return Err(Error::bitcode("invalid parameter attribute group"));
    }
    let group_id = record.operands[0];
    let index = u32::try_from(record.operands[1])
        .map_err(|_| Error::bitcode("parameter attribute index overflow"))?;
    let is_aarch64 = is_aarch64_triple(target_triple);
    let mut attrs = Vec::new();
    let mut legacy_memory = MemoryEffects::unknown();
    let mut legacy_memory_seen = false;
    let mut modern_memory = None;
    let mut cursor = 2;
    while cursor < record.operands.len() {
        match record.operands[cursor] {
            0 => {
                let code = *record
                    .operands
                    .get(cursor + 1)
                    .ok_or_else(|| Error::bitcode("truncated enum attribute"))?;
                if index == FUNCTION_INDEX {
                    if let Some(effect) = legacy_memory_effect(code) {
                        legacy_memory = legacy_memory.intersect(effect);
                        legacy_memory_seen = true;
                        cursor += 2;
                        continue;
                    }
                }
                attrs.push(Attribute(decode_enum_attribute(code, index)?.to_string()));
                cursor += 2;
            }
            1 => {
                let code = *record
                    .operands
                    .get(cursor + 1)
                    .ok_or_else(|| Error::bitcode("truncated integer attribute"))?;
                let value = *record
                    .operands
                    .get(cursor + 2)
                    .ok_or_else(|| Error::bitcode("truncated integer attribute"))?;
                if code == 86 {
                    let effects = MemoryEffects::from_int(value, is_aarch64)?;
                    if index == FUNCTION_INDEX {
                        modern_memory = Some(effects);
                    } else {
                        attrs.push(Attribute(effects.text()));
                    }
                    cursor += 3;
                    continue;
                }
                attrs.push(decode_int_attribute(code, value)?);
                cursor += 3;
            }
            3 | 4 => {
                let has_value = record.operands[cursor] == 4;
                cursor += 1;
                let kind = read_c_string(&record.operands, &mut cursor)?;
                let value = if has_value {
                    Some(read_c_string(&record.operands, &mut cursor)?)
                } else {
                    None
                };
                let kind = quote_bytes(&kind);
                attrs.push(Attribute(match value {
                    Some(value) => format!("{kind}={}", quote_bytes(&value)),
                    None => kind,
                }));
            }
            5 | 6 => {
                let has_type = record.operands[cursor] == 6;
                let code = *record
                    .operands
                    .get(cursor + 1)
                    .ok_or_else(|| Error::bitcode("truncated type attribute"))?;
                cursor += 2;
                let name = type_attribute_name(code)?;
                if has_type {
                    let id = *record
                        .operands
                        .get(cursor)
                        .ok_or_else(|| Error::bitcode("truncated type attribute"))?;
                    cursor += 1;
                    let ty = *type_ids.get(id as usize).ok_or_else(|| {
                        Error::bitcode("type attribute references an unknown type")
                    })?;
                    attrs.push(Attribute(format!("{name}({})", types.display(ty))));
                } else {
                    attrs.push(Attribute(name.to_string()));
                }
            }
            other => {
                return Err(Error::bitcode(format!(
                    "unsupported parameter attribute encoding {other}"
                )));
            }
        }
    }
    if legacy_memory_seen {
        modern_memory = Some(legacy_memory);
    }
    if let Some(effects) = modern_memory {
        insert_memory_attribute(&mut attrs, Attribute(effects.text()));
    }
    Ok((group_id, index, attrs))
}

fn read_c_string(operands: &[u64], cursor: &mut usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(value) = operands.get(*cursor) {
        *cursor += 1;
        if *value == 0 {
            return Ok(bytes);
        }
        bytes.push(*value as u8);
    }
    Err(Error::bitcode("unterminated attribute string"))
}

fn decode_enum_attribute(code: u64, index: u32) -> Result<&'static str> {
    if index != FUNCTION_INDEX {
        match code {
            20 => return Ok("readnone"),
            21 => return Ok("readonly"),
            52 => return Ok("writeonly"),
            _ => {}
        }
    } else {
        match code {
            20 => return Ok("memory(none)"),
            21 => return Ok("memory(read)"),
            52 => return Ok("memory(write)"),
            _ => {}
        }
    }
    ENUM_ATTRIBUTES
        .iter()
        .find_map(|(name, value)| (*value == code).then_some(*name))
        .ok_or_else(|| Error::bitcode(format!("unsupported enum attribute code {code}")))
}

fn type_attribute_name(code: u64) -> Result<&'static str> {
    match code {
        3 => Ok("byval"),
        29 => Ok("sret"),
        38 => Ok("inalloca"),
        65 => Ok("preallocated"),
        69 => Ok("byref"),
        77 => Ok("elementtype"),
        _ => Err(Error::bitcode(format!(
            "unsupported type attribute code {code}"
        ))),
    }
}

fn decode_int_attribute(code: u64, value: u64) -> Result<Attribute> {
    Ok(Attribute(match code {
        1 => format!("align {value}"),
        25 => format!("alignstack({value})"),
        41 => format!("dereferenceable({value})"),
        42 => format!("dereferenceable_or_null({value})"),
        _ => {
            return Err(Error::bitcode(format!(
                "unsupported integer attribute code {code}"
            )));
        }
    }))
}

/// The index used for function-level attributes in a PARAMATTR list.
pub(crate) const fn function_attr_index() -> u32 {
    FUNCTION_INDEX
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(values: &[&str]) -> Vec<Attribute> {
        values
            .iter()
            .map(|value| Attribute((*value).to_string()))
            .collect()
    }

    #[test]
    fn merge_replaces_same_kind_or_string_key() {
        let mut existing = attrs(&[
            "align 8",
            "noundef",
            "\"key\"=\"a\"",
            "byval(%A)",
            "dereferenceable(8)",
        ]);
        merge_attributes(
            &mut existing,
            &attrs(&[
                "align 16",
                "\"key\"=\"b\"",
                "byval(%B)",
                "dereferenceable_or_null(16)",
            ]),
        );
        assert_eq!(
            existing,
            attrs(&[
                "align 16",
                "noundef",
                "\"key\"=\"b\"",
                "byval(%B)",
                "dereferenceable(8)",
                "dereferenceable_or_null(16)",
            ])
        );
    }

    #[test]
    fn merge_keeps_last_duplicate_from_incoming() {
        let mut existing = attrs(&["align 8"]);
        merge_attributes(&mut existing, &attrs(&["align 16", "align 32"]));
        assert_eq!(existing, attrs(&["align 32"]));
    }

    #[test]
    fn memory_attributes_merge_by_kind_but_legacy_flags_stay_distinct() {
        let mut existing = attrs(&["memory(none)", "noundef"]);
        merge_attributes(&mut existing, &attrs(&["memory(read)"]));
        assert_eq!(existing, attrs(&["memory(read)", "noundef"]));

        let mut parameter = attrs(&["readnone"]);
        merge_attributes(&mut parameter, &attrs(&["memory(none)"]));
        assert_eq!(parameter, attrs(&["readnone", "memory(none)"]));
    }

    #[test]
    fn legacy_function_memory_effects_intersect() {
        let mut effects = MemoryEffects::unknown();
        for code in [20, 21, 20] {
            effects = effects.intersect(legacy_memory_effect(code).unwrap());
        }
        assert_eq!(effects.text(), "memory(none)");

        let mut effects = MemoryEffects::unknown();
        for code in [21, 21] {
            effects = effects.intersect(legacy_memory_effect(code).unwrap());
        }
        assert_eq!(effects.text(), "memory(read)");

        let mut effects = MemoryEffects::unknown();
        for code in [21, 52] {
            effects = effects.intersect(legacy_memory_effect(code).unwrap());
        }
        assert_eq!(effects.text(), "memory(none)");
    }

    #[test]
    fn decodes_legacy_function_memory_intersection() {
        let record = Record {
            code: PARAMATTR_GRP_CODE_ENTRY,
            operands: vec![1, u64::from(u32::MAX), 0, 20, 0, 21, 0, 20],
            blob: None,
        };
        let (_, index, attrs) =
            decode_attribute_group(&record, &TypeTable::new(), &[], None).expect("decode");
        assert_eq!(index, u32::MAX);
        assert_eq!(attrs, vec![Attribute("memory(none)".to_string())]);
    }

    #[test]
    fn llvm19_memory_encoding_uses_three_locations() {
        let encode = |text: &str| encode_attribute(&Attribute(text.to_string())).unwrap();
        assert_eq!(encode("memory(read)"), vec![1, 86, 21]);
        assert_eq!(encode("memory(write)"), vec![1, 86, 42]);
        assert_eq!(
            encode("memory(argmem: read, inaccessiblemem: write)"),
            vec![1, 86, 9]
        );
        assert_eq!(
            MemoryEffects::from_int(21, false).unwrap().text(),
            "memory(read)"
        );
        assert_eq!(
            MemoryEffects::from_int((2u64 << 56) | 0x45, false)
                .unwrap()
                .text(),
            "memory(read)"
        );
        assert_eq!(
            MemoryEffects::from_int((2u64 << 56) | 0x555, false)
                .unwrap()
                .text(),
            "memory(read)"
        );
        assert_eq!(encode("memory(read, errnomem: read)"), vec![1, 86, 21]);
    }

    #[test]
    fn rejects_unrepresentable_memory_locations() {
        assert!(parse_memory_attribute("memory(errnomem: read)").is_none());
        assert!(parse_memory_attribute("memory(target_mem0: write)").is_none());
        assert_eq!(
            MemoryEffects::from_int((1u64 << 56) | (1 << 4), false)
                .unwrap()
                .text(),
            "memory(read, argmem: none, inaccessiblemem: none)"
        );
        assert!(MemoryEffects::from_int((2u64 << 56) | (1 << 9), false).is_err());
        assert_eq!(
            parse_memory_attribute("memory(read, errnomem: read)")
                .unwrap()
                .text(),
            "memory(read)"
        );
        assert!(parse_memory_attribute("memory(read, errnomem: none)").is_none());
    }

    #[test]
    fn decodes_version_two_memory_but_rejects_target_effects() {
        let record = |value| Record {
            code: PARAMATTR_GRP_CODE_ENTRY,
            operands: vec![1, u64::from(u32::MAX), 1, 86, value],
            blob: None,
        };
        let (_, _, attrs) =
            decode_attribute_group(&record((2u64 << 56) | 0x555), &TypeTable::new(), &[], None)
                .expect("version 2 read");
        assert_eq!(attrs, vec![Attribute("memory(read)".to_string())]);
        assert!(
            decode_attribute_group(
                &record((2u64 << 56) | (1 << 9)),
                &TypeTable::new(),
                &[],
                None
            )
            .is_err()
        );
    }

    #[test]
    fn aarch64_rejects_memory_effects_that_lose_target_access() {
        let aarch64 = Some("aarch64-unknown-linux-gnu");
        let resolve =
            |text: &str| resolve_function_attributes(vec![Attribute(text.to_string())], aarch64);
        assert!(resolve("memory(write, inaccessiblemem: none, errnomem: write)").is_err());
        assert!(resolve("memory(write, inaccessiblemem: none)").is_err());
        assert_eq!(
            resolve("memory(write, inaccessiblemem: write, errnomem: write)").unwrap(),
            vec![Attribute("memory(write)".to_string())]
        );
        assert_eq!(
            resolve("memory(inaccessiblemem: read, target_mem0: read, target_mem1: read)").unwrap(),
            vec![Attribute("memory(inaccessiblemem: read)".to_string())]
        );
        assert!(
            resolve_function_attributes(
                vec![Attribute(
                    "memory(inaccessiblemem: read, target_mem0: read, target_mem1: read)"
                        .to_string()
                )],
                Some("x86_64-unknown-linux-gnu")
            )
            .is_err()
        );
    }

    #[test]
    fn aarch64_target_memory_matches_inaccessible() {
        let encoded = (2u64 << 56) | 0x504;
        assert_eq!(
            MemoryEffects::from_int(encoded, true).unwrap().text(),
            "memory(inaccessiblemem: read)"
        );
        assert!(MemoryEffects::from_int(encoded, false).is_err());
        let record = Record {
            code: PARAMATTR_GRP_CODE_ENTRY,
            operands: vec![1, u64::from(u32::MAX), 1, 86, encoded],
            blob: None,
        };
        let (_, _, attrs) = decode_attribute_group(
            &record,
            &TypeTable::new(),
            &[],
            Some("aarch64-unknown-linux-gnu"),
        )
        .expect("aarch64 target memory");
        assert_eq!(
            attrs,
            vec![Attribute("memory(inaccessiblemem: read)".to_string())]
        );
    }

    #[test]
    fn modern_memory_attributes_round_trip_through_text() {
        for text in [
            "memory(none)",
            "memory(read)",
            "memory(write)",
            "memory(readwrite)",
            "memory(argmem: read)",
            "memory(inaccessiblemem: write)",
            "memory(argmem: read, inaccessiblemem: write)",
        ] {
            let effects = parse_memory_attribute(text).expect(text);
            assert_eq!(effects.text(), text);
        }
    }

    #[test]
    fn deduplicate_keeps_last_value_in_first_position() {
        let mut values = attrs(&["align 8", "noundef", "align 16", r#""k"="a""#, r#""k"="b""#]);
        deduplicate_attributes(&mut values);
        assert_eq!(values, attrs(&["align 16", "noundef", r#""k"="b""#]));
    }
}

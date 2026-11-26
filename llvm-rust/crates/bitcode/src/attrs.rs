//! Attribute-list encoding for the LLVM bitcode subset.
//!
//! The IR keeps attributes as their textual spelling.  This module translates
//! the spellings we support to and from the PARAMATTR records used by LLVM.

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
        } else {
            joined.push(parts[index].clone());
            index += 1;
        }
    }
    Ok(joined)
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
    if let Some(value) = text.strip_prefix("alignstack ").or_else(|| {
        text.strip_prefix("alignstack(")
            .and_then(|value| value.strip_suffix(')'))
    }) {
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
    if let Some(value) = text
        .strip_prefix("memory(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let code = match value {
            "none" => 20,
            "read" => 21,
            "write" => 52,
            _ => {
                return Err(Error::bitcode(format!(
                    "unsupported memory attribute `{text}`"
                )));
            }
        };
        return Ok(vec![0, code]);
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
    ("readnone", 20),
    ("readonly", 21),
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
    ("writeonly", 52),
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
    ENUM_ATTRIBUTES
        .iter()
        .find_map(|(name, code)| (*name == text).then_some(*code))
}

/// Decodes one PARAMATTR group entry.
pub(crate) fn decode_attribute_group(
    record: &Record,
    types: &TypeTable,
    type_ids: &[TypeId],
) -> Result<(u64, u32, Vec<Attribute>)> {
    if record.operands.len() < 3 {
        return Err(Error::bitcode("invalid parameter attribute group"));
    }
    let group_id = record.operands[0];
    let index = u32::try_from(record.operands[1])
        .map_err(|_| Error::bitcode("parameter attribute index overflow"))?;
    let mut attrs = Vec::new();
    let mut cursor = 2;
    while cursor < record.operands.len() {
        match record.operands[cursor] {
            0 => {
                let code = *record
                    .operands
                    .get(cursor + 1)
                    .ok_or_else(|| Error::bitcode("truncated enum attribute"))?;
                attrs.push(Attribute(decode_enum_attribute(code)?.to_string()));
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

fn decode_enum_attribute(code: u64) -> Result<&'static str> {
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
        25 => format!("alignstack {value}"),
        41 => format!("dereferenceable({value})"),
        42 => format!("dereferenceable_or_null({value})"),
        86 => match value {
            0 => "readnone".to_string(),
            21 => "readonly".to_string(),
            42 => "writeonly".to_string(),
            _ => {
                return Err(Error::bitcode(format!(
                    "unsupported memory attribute encoding {value}"
                )));
            }
        },
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

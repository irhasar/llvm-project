//! Attribute-list encoding for the LLVM bitcode subset.
//!
//! The IR keeps attributes as their textual spelling.  This module translates
//! the spellings we support to and from the PARAMATTR records used by LLVM.

use llvm_rust_ir::{Attribute, Module};
use llvm_rust_support::{Error, Result};

use crate::bitstream::Record;

pub(crate) const PARAMATTR_CODE_ENTRY: u64 = 2;
pub(crate) const PARAMATTR_GRP_CODE_ENTRY: u64 = 3;

const FUNCTION_INDEX: u32 = u32::MAX;

/// Expands `#N` references and returns the concrete attributes in `attrs`.
pub(crate) fn expand_attributes(module: &Module, attrs: &[Attribute]) -> Result<Vec<Attribute>> {
    let mut expanded = Vec::new();
    for attr in attrs {
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
        operands.extend(kind.bytes().map(u64::from));
        operands.push(0);
        if let Some(value) = value {
            operands.extend(value.bytes().map(u64::from));
            operands.push(0);
        }
        return Ok(operands);
    }

    if let Some(value) = text.strip_prefix("align ") {
        return Ok(vec![1, 1, parse_u64(value, "align")?]);
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

fn parse_string_attribute(text: &str) -> Result<Option<(String, Option<String>)>> {
    if !text.starts_with('"') {
        return Ok(None);
    }
    let end = text[1..]
        .find('"')
        .map(|offset| offset + 1)
        .ok_or_else(|| Error::bitcode("unterminated string attribute"))?;
    let kind = text[1..end].to_string();
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
    Ok(Some((kind, Some(rest[1..rest.len() - 1].to_string()))))
}

fn parse_u64(text: &str, name: &str) -> Result<u64> {
    text.parse::<u64>()
        .map_err(|_| Error::bitcode(format!("invalid `{name}` value `{text}`")))
}

fn enum_attribute_code(text: &str) -> Option<u64> {
    Some(match text {
        "alwaysinline" => 2,
        "inreg" => 5,
        "minsize" => 6,
        "naked" => 7,
        "nest" => 8,
        "noalias" => 9,
        "nobuiltin" => 10,
        "nocapture" => 11,
        "noduplicate" => 12,
        "noinline" => 14,
        "nonlazybind" => 15,
        "noredzone" => 16,
        "noreturn" => 17,
        "nounwind" => 18,
        "optsize" => 19,
        "readnone" => 20,
        "readonly" => 21,
        "returned" => 22,
        "returns_twice" => 23,
        "signext" => 24,
        "ssp" => 26,
        "sspreq" => 27,
        "sspstrong" => 28,
        "sanitize_address" => 30,
        "sanitize_thread" => 31,
        "sanitize_memory" => 32,
        "builtin" => 35,
        "cold" => 36,
        "optnone" => 37,
        "nonnull" => 39,
        "jumptable" => 40,
        "convergent" => 43,
        "safestack" => 44,
        "swiftself" => 46,
        "swifterror" => 47,
        "norecurse" => 48,
        "writeonly" => 52,
        "speculatable" => 53,
        "strictfp" => 54,
        "sanitize_hwaddress" => 55,
        "nocf_check" => 56,
        "optforfuzzing" => 57,
        "speculative_load_hardening" => 59,
        "immarg" => 60,
        "willreturn" => 61,
        "nofree" => 62,
        "nosync" => 63,
        "sanitize_memtag" => 64,
        "preallocated" => 65,
        "nomerge" => 66,
        "null_pointer_is_valid" => 67,
        "noundef" => 68,
        "mustprogress" => 70,
        "nocallback" => 71,
        "hot" => 72,
        "noprofile" => 73,
        "swiftasync" => 75,
        "nosanitize_coverage" => 76,
        "disable_sanitizer_instrumentation" => 78,
        "nosanitize_bounds" => 79,
        "presplitcoroutine" => 83,
        "fnretthunk_extern" => 84,
        "skipprofile" => 85,
        "optimizefordebugging" => 88,
        "dead_on_unwind" => 91,
        "sanitize_numerical_stability" => 93,
        "hybrid_patchable" => 95,
        "sanitize_realtime" => 96,
        "sanitize_realtime_blocking" => 97,
        "coro_elide_safe" => 98,
        "no_ext" => 99,
        "no_divergence_source" => 100,
        "sanitize_type" => 101,
        "dead_on_return" => 103,
        "sanitize_alloc_token" => 104,
        "no_create_undef_or_poison" => 105,
        "nooutline" => 107,
        "flatten" => 108,
        "noipa" => 109,
        _ => return None,
    })
}

/// Decodes one PARAMATTR group entry.
pub(crate) fn decode_attribute_group(record: &Record) -> Result<(u64, u32, Vec<Attribute>)> {
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
                attrs.push(Attribute(match value {
                    Some(value) => format!("\"{kind}\"=\"{value}\""),
                    None => format!("\"{kind}\""),
                }));
            }
            5 | 6 => {
                return Err(Error::bitcode(
                    "type attributes are not supported by the bitcode reader yet",
                ));
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

fn read_c_string(operands: &[u64], cursor: &mut usize) -> Result<String> {
    let mut bytes = Vec::new();
    while let Some(value) = operands.get(*cursor) {
        *cursor += 1;
        if *value == 0 {
            return String::from_utf8(bytes)
                .map_err(|_| Error::bitcode("attribute string is not UTF-8"));
        }
        bytes.push(*value as u8);
    }
    Err(Error::bitcode("unterminated attribute string"))
}

fn decode_enum_attribute(code: u64) -> Result<&'static str> {
    Ok(match code {
        2 => "alwaysinline",
        5 => "inreg",
        6 => "minsize",
        7 => "naked",
        8 => "nest",
        9 => "noalias",
        10 => "nobuiltin",
        11 => "nocapture",
        12 => "noduplicate",
        14 => "noinline",
        15 => "nonlazybind",
        16 => "noredzone",
        17 => "noreturn",
        18 => "nounwind",
        19 => "optsize",
        20 => "readnone",
        21 => "readonly",
        22 => "returned",
        23 => "returns_twice",
        24 => "signext",
        26 => "ssp",
        27 => "sspreq",
        28 => "sspstrong",
        30 => "sanitize_address",
        31 => "sanitize_thread",
        32 => "sanitize_memory",
        35 => "builtin",
        36 => "cold",
        37 => "optnone",
        39 => "nonnull",
        40 => "jumptable",
        43 => "convergent",
        44 => "safestack",
        46 => "swiftself",
        47 => "swifterror",
        48 => "norecurse",
        52 => "writeonly",
        53 => "speculatable",
        54 => "strictfp",
        55 => "sanitize_hwaddress",
        56 => "nocf_check",
        57 => "optforfuzzing",
        59 => "speculative_load_hardening",
        60 => "immarg",
        61 => "willreturn",
        62 => "nofree",
        63 => "nosync",
        64 => "sanitize_memtag",
        65 => "preallocated",
        66 => "nomerge",
        67 => "null_pointer_is_valid",
        68 => "noundef",
        70 => "mustprogress",
        71 => "nocallback",
        72 => "hot",
        73 => "noprofile",
        75 => "swiftasync",
        76 => "nosanitize_coverage",
        78 => "disable_sanitizer_instrumentation",
        79 => "nosanitize_bounds",
        83 => "presplitcoroutine",
        84 => "fnretthunk_extern",
        85 => "skipprofile",
        88 => "optimizefordebugging",
        91 => "dead_on_unwind",
        93 => "sanitize_numerical_stability",
        95 => "hybrid_patchable",
        96 => "sanitize_realtime",
        97 => "sanitize_realtime_blocking",
        98 => "coro_elide_safe",
        99 => "no_ext",
        100 => "no_divergence_source",
        101 => "sanitize_type",
        103 => "dead_on_return",
        104 => "sanitize_alloc_token",
        105 => "no_create_undef_or_poison",
        107 => "nooutline",
        108 => "flatten",
        109 => "noipa",
        _ => {
            return Err(Error::bitcode(format!(
                "unsupported enum attribute code {code}"
            )));
        }
    })
}

fn decode_int_attribute(code: u64, value: u64) -> Result<Attribute> {
    Ok(Attribute(match code {
        1 => format!("align {value}"),
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

//! LLVM identifier quoting.

use core::fmt;

/// Returns true when `name` can be printed without quotes.
pub fn is_unquoted_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if name.bytes().all(|byte| byte.is_ascii_digit()) {
        return true;
    }
    let mut bytes = name.bytes();
    let first = bytes.next().expect("checked non-empty");
    if !is_name_start(first) {
        return false;
    }
    bytes.all(is_name_char)
}

fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'-' | b'$' | b'.' | b'_')
}

fn is_name_char(byte: u8) -> bool {
    is_name_start(byte) || byte.is_ascii_digit()
}

/// Writes `name`, adding quotes and escapes when required by LLVM syntax.
pub fn write_name(f: &mut impl fmt::Write, name: &str) -> fmt::Result {
    if is_unquoted_name(name) {
        return f.write_str(name);
    }
    f.write_char('"')?;
    for byte in name.bytes() {
        match byte {
            b'"' => f.write_str("\\22")?,
            b'\\' => f.write_str("\\5C")?,
            0x20..=0x7e => f.write_char(byte as char)?,
            _ => write!(f, "\\{byte:02X}")?,
        }
    }
    f.write_char('"')
}

/// Returns `name` quoted only when needed.
pub fn quote_name(name: &str) -> String {
    let mut out = String::new();
    write_name(&mut out, name).expect("writing to String cannot fail");
    out
}

/// Decodes the contents of a quoted LLVM name (without the quotes).
pub fn unescape_name(text: &str) -> Option<String> {
    String::from_utf8(unescape_bytes(text)?).ok()
}

/// Decodes LLVM `\XX` escapes to their underlying bytes.
pub fn unescape_bytes(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        let byte = bytes[idx];
        if byte != b'\\' {
            out.push(byte);
            idx += 1;
            continue;
        }
        if idx + 1 >= bytes.len() {
            return None;
        }
        let high = hex_value(bytes[idx + 1])?;
        if idx + 2 >= bytes.len() {
            return None;
        }
        let low = hex_value(bytes[idx + 2])?;
        out.push((high << 4) | low);
        idx += 3;
    }
    Some(out)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_names_are_not_quoted() {
        assert!(is_unquoted_name("main"));
        assert!(is_unquoted_name("llvm.memcpy"));
        assert!(is_unquoted_name("42"));
        assert!(!is_unquoted_name(""));
        assert!(!is_unquoted_name("a b"));
        assert!(!is_unquoted_name("1abc"));
    }

    #[test]
    fn quotes_and_escapes() {
        assert_eq!(quote_name("main"), "main");
        assert_eq!(quote_name("a b"), "\"a b\"");
        assert_eq!(quote_name("a\"b"), "\"a\\22b\"");
        assert_eq!(quote_name("a\\b"), "\"a\\5Cb\"");
    }

    #[test]
    fn unescapes_quoted_names() {
        assert_eq!(unescape_name("a\\20b").as_deref(), Some("a b"));
        assert_eq!(unescape_name("a\\22b").as_deref(), Some("a\"b"));
        assert_eq!(unescape_name("bad\\2"), None);
        assert_eq!(unescape_name("bad\\zz"), None);
    }
}

/// Formats a byte string as an LLVM string literal, including quotes.
pub fn quote_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('"');
    for &byte in bytes {
        match byte {
            b'"' => out.push_str("\\22"),
            b'\\' => out.push_str("\\5C"),
            0x20..=0x7e => out.push(byte as char),
            _ => out.push_str(&format!("\\{byte:02X}")),
        }
    }
    out.push('"');
    out
}

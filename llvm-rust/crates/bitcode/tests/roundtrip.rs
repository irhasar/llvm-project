use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use llvm_rust_bitcode::{read_bitcode, write_bitcode};
use llvm_rust_ir::{parse_module, print_module};

fn corpus() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus");
    let mut files: Vec<PathBuf> = fs::read_dir(root)
        .expect("corpus directory")
        .map(|entry| entry.expect("corpus entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ll"))
        .collect();
    files.sort();
    files
}

/// Replaces `#N` attribute-group references with the group body so that a
/// round trip through bitcode can be compared without preserving the original
/// grouping layout.
/// Rewrites group-only spellings (`alignstack=16`) to the inline spelling
/// (`alignstack(16)`) used after groups are expanded.
fn normalize_group_body(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(index) = rest.find("alignstack=") {
        out.push_str(&rest[..index]);
        let tail = &rest[index + "alignstack=".len()..];
        let digits = tail.chars().take_while(|ch| ch.is_ascii_digit()).count();
        out.push_str("alignstack(");
        out.push_str(&tail[..digits]);
        out.push(')');
        rest = &tail[digits..];
    }
    out.push_str(rest);
    out.replace("align=", "align ")
}

fn inline_attr_groups(text: &str) -> String {
    let mut groups = HashMap::new();
    let mut body = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("attributes #")
            && let Some((id, tail)) = rest.split_once(" = { ")
            && let Some(inner) = tail.strip_suffix(" }")
        {
            groups.insert(format!("#{id}"), normalize_group_body(inner));
            continue;
        }
        body.push(line);
    }
    let result = collapse_alignments(replace_group_refs(&body.join("\n"), &groups));
    result.trim_end().to_string()
}

fn collapse_alignments(mut text: String) -> String {
    let mut search = 0;
    while let Some(relative) = text[search..].find("align ") {
        let start = search + relative;
        let value_start = start + "align ".len();
        let value_end = text[value_start..]
            .find(|ch: char| !ch.is_ascii_digit())
            .map(|offset| value_start + offset)
            .unwrap_or(text.len());
        let tail_start =
            value_end + (text[value_end..].len() - text[value_end..].trim_start().len());
        if text[tail_start..].starts_with("align ") {
            text.replace_range(start..tail_start, "");
            search = start;
        } else {
            search = value_end;
        }
    }
    text
}

fn replace_group_refs(text: &str, groups: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut chars = text.char_indices().peekable();
    let mut quoted = false;
    while let Some((index, ch)) = chars.next() {
        if ch == '"' {
            quoted = !quoted;
            out.push(ch);
            continue;
        }
        if ch == '\\' && quoted {
            out.push(ch);
            if let Some((_, next)) = chars.next() {
                out.push(next);
            }
            continue;
        }
        if ch == '#' && !quoted {
            let mut end = index + 1;
            for (offset, next) in text[index + 1..].char_indices() {
                if !next.is_ascii_digit() {
                    break;
                }
                end = index + 1 + offset + next.len_utf8();
            }
            let reference = &text[index..end];
            if let Some(expansion) = groups.get(reference) {
                out.push_str(expansion);
                while let Some((next_index, _)) = chars.peek() {
                    if *next_index < end {
                        chars.next();
                    } else {
                        break;
                    }
                }
                continue;
            }
        }
        out.push(ch);
    }
    out
}

#[test]
fn rust_bitcode_round_trip_matches_text() {
    let files = corpus();
    assert!(!files.is_empty(), "corpus is empty");
    for path in files {
        let source = fs::read_to_string(&path).expect("read corpus file");
        let mut module = parse_module(&source).expect("parse corpus file");
        let before = print_module(&module);
        let bytes = write_bitcode(&mut module).expect("write bitcode");
        let read = read_bitcode(&bytes).expect("read bitcode");
        let after = print_module(&read);
        assert_eq!(
            inline_attr_groups(&before),
            inline_attr_groups(&after),
            "round trip changed {}",
            path.display()
        );
    }
}

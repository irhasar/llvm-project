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
fn inline_attr_groups(text: &str) -> String {
    let mut groups = HashMap::new();
    let mut body = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("attributes #")
            && let Some((id, tail)) = rest.split_once(" = { ")
            && let Some(inner) = tail.strip_suffix(" }")
        {
            groups.insert(format!("#{id}"), inner.to_string());
            continue;
        }
        body.push(line);
    }
    let mut result = body.join("\n");
    for (reference, expansion) in groups {
        result = result.replace(&reference, &expansion);
    }
    result.trim_end().to_string()
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

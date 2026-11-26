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
        assert_eq!(before, after, "round trip changed {}", path.display());
    }
}

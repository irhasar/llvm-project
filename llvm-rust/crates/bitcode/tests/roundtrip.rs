use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use llvm_rust_bitcode::{
    ReadOptions, WriteOptions, read_bitcode, read_bitcode_with_options, write_bitcode,
    write_bitcode_with_options,
};
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
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut quoted = false;
    let mut escaped = false;
    while index < bytes.len() {
        let ch = text[index..]
            .chars()
            .next()
            .expect("index is a char boundary");
        if quoted {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            index += ch.len_utf8();
            continue;
        }
        if ch == '"' {
            quoted = true;
            out.push('"');
            index += 1;
            continue;
        }
        if text[index..].starts_with("alignstack=") {
            let value_start = index + "alignstack=".len();
            let mut value_end = value_start;
            while value_end < bytes.len() && bytes[value_end].is_ascii_digit() {
                value_end += 1;
            }
            if value_end > value_start {
                out.push_str("alignstack(");
                out.push_str(&text[value_start..value_end]);
                out.push(')');
                index = value_end;
                continue;
            }
        }
        if text[index..].starts_with("align=") {
            out.push_str("align ");
            index += "align=".len();
            continue;
        }
        out.push(ch);
        index += ch.len_utf8();
    }
    out
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

fn collapse_alignments(text: String) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut quoted = false;
    let mut escaped = false;
    while index < bytes.len() {
        let ch = text[index..]
            .chars()
            .next()
            .expect("index is a char boundary");
        if quoted {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            index += ch.len_utf8();
            continue;
        }
        if ch == '"' {
            quoted = true;
            out.push('"');
            index += 1;
            continue;
        }
        if text[index..].starts_with("align ") {
            let value_start = index + "align ".len();
            let mut value_end = value_start;
            while value_end < bytes.len() && bytes[value_end].is_ascii_digit() {
                value_end += 1;
            }
            if value_end > value_start {
                let mut tail_start = value_end;
                while tail_start < bytes.len() && bytes[tail_start].is_ascii_whitespace() {
                    tail_start += 1;
                }
                if text[tail_start..].starts_with("align ") {
                    index = tail_start;
                    continue;
                }
            }
        }
        out.push(ch);
        index += ch.len_utf8();
    }
    out
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
        // Repeated memory effects are canonicalized, and the AArch64 fixture
        // uses LLVM 19 semantics covered by the mode-specific test below.
        if path.file_name().is_some_and(|name| {
            name == "memory_effects.ll" || name == "memory_groups.ll" || name == "aarch64_memory.ll"
        }) {
            continue;
        }
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

#[test]
fn scoped_memory_overrides_are_visible_after_round_trip() {
    let source = r#"declare void @callee()
declare void @inline_legacy(ptr) readnone #0
declare void @inline_modern(ptr) memory(argmem: write) #1

define void @caller() {
entry:
  call void @callee() readnone #0
  ret void
}

attributes #0 = { memory(argmem: write) }
attributes #1 = { readnone }
"#;
    let mut module = parse_module(source).expect("parse scoped memory");
    let bytes = write_bitcode(&mut module).expect("write scoped memory");
    let read = read_bitcode(&bytes).expect("read scoped memory");
    let text = print_module(&read);
    assert!(
        text.contains("declare void @inline_legacy(ptr) memory(argmem: write)"),
        "{text}"
    );
    assert!(
        text.contains("declare void @inline_modern(ptr) memory(none)"),
        "{text}"
    );
    assert!(
        text.contains("call void @callee() memory(argmem: write)"),
        "{text}"
    );
}

#[test]
fn aarch64_target_memory_survives_disassemble_assemble() {
    let source = r#"target triple = "aarch64-unknown-linux-gnu"
declare void @callee()
declare void @read_default_write_inaccessible() memory(read, inaccessiblemem: write, target_mem: write)

define void @caller() {
entry:
  call void @callee() memory(read, inaccessiblemem: write, target_mem: write)
  ret void
}
"#;
    let mut module = parse_module(source).expect("parse AArch64 memory");
    let bytes = write_bitcode(&mut module).expect("write AArch64 memory");
    let read = read_bitcode(&bytes).expect("read AArch64 memory");
    let text = print_module(&read);
    assert!(
        text.contains(
            "declare void @read_default_write_inaccessible() memory(read, inaccessiblemem: write, target_mem: write)"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "call void @callee() memory(read, inaccessiblemem: write, target_mem: write)"
        ),
        "{text}"
    );

    let mut reparsed = parse_module(&text).expect("reparse AArch64 disassembly");
    let second = write_bitcode(&mut reparsed).expect("rewrite AArch64 memory");
    let second_read = read_bitcode(&second).expect("reread AArch64 memory");
    assert_eq!(text, print_module(&second_read));
}

#[test]
fn llvm19_mode_omits_aarch64_target_clauses() {
    let source = r#"target triple = "aarch64-unknown-linux-gnu"
declare void @callee()
declare void @read_default_write_inaccessible() memory(read, inaccessiblemem: write)

define void @caller() {
entry:
  call void @callee() memory(read, inaccessiblemem: write)
  ret void
}
"#;
    let mut module = parse_module(source).expect("parse LLVM 19 AArch64 memory");
    let bytes = write_bitcode_with_options(
        &mut module,
        WriteOptions {
            llvm19_compat: true,
        },
    )
    .expect("write LLVM 19 AArch64 memory");
    let llvm19 = print_module(
        &read_bitcode_with_options(
            &bytes,
            ReadOptions {
                llvm19_compat: true,
            },
        )
        .expect("read LLVM 19 AArch64 memory"),
    );
    assert!(!llvm19.contains("target_mem"), "{llvm19}");
    assert!(
        llvm19.contains(
            "declare void @read_default_write_inaccessible() memory(read, inaccessiblemem: write)"
        ),
        "{llvm19}"
    );
    assert!(
        llvm19.contains("call void @callee() memory(read, inaccessiblemem: write)"),
        "{llvm19}"
    );

    let target_aware = print_module(&read_bitcode(&bytes).expect("read target-aware"));
    assert!(target_aware.contains("target_mem: write"), "{target_aware}");

    let mut reparsed = parse_module(&llvm19).expect("reparse LLVM 19 disassembly");
    write_bitcode_with_options(
        &mut reparsed,
        WriteOptions {
            llvm19_compat: true,
        },
    )
    .expect("reassemble LLVM 19 disassembly");
}

#[test]
fn disassemble_assemble_preserves_unnamed_slots() {
    let source = "declare void @0()\n\n@1 = global i32 1\n";
    let mut module = parse_module(source).expect("parse numeric slots");
    let text = print_module(&module);
    assert!(text.contains("@0 = global i32 1"), "{text}");
    assert!(text.contains("declare void @1()"), "{text}");

    let bytes = write_bitcode(&mut module).expect("write bitcode");
    let read = read_bitcode(&bytes).expect("read bitcode");
    let after = print_module(&read);
    assert_eq!(text, after);
    assert!(
        !after.contains("@\"0\""),
        "slot became a quoted name: {after}"
    );

    let mut reparsed = parse_module(&after).expect("reparse disassembly");
    let second = write_bitcode(&mut reparsed).expect("rewrite bitcode");
    let second_read = read_bitcode(&second).expect("reread bitcode");
    assert_eq!(after, print_module(&second_read));
}

#[test]
fn explicit_numeric_names_stay_quoted() {
    let source = "declare void @\"0\"()\n";
    let module = parse_module(source).expect("parse quoted numeric name");
    let text = print_module(&module);
    assert!(text.contains("declare void @\"0\"()"), "{text}");
}

#[test]
fn shared_attribute_lists_are_not_charged_per_declaration() {
    let params = (0..128)
        .map(|index| {
            format!("ptr noalias nocapture nonnull readonly align 8 dereferenceable(8) %p{index}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let source = (0..100)
        .map(|index| format!("declare void @f{index}({params})"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut module = parse_module(&source).expect("parse shared attribute corpus");
    let bytes = write_bitcode(&mut module).expect("write shared attribute corpus");
    assert!(
        bytes.len() < 100_000,
        "writer did not share attribute lists"
    );
    let read = read_bitcode(&bytes).expect("read shared attribute corpus");
    assert_eq!(read.functions.len(), 100);
    let first = read.functions.iter().next().expect("function").1.params[0]
        .attrs
        .clone();
    for (_, function) in read.functions.iter() {
        assert!(std::sync::Arc::ptr_eq(&first, &function.params[0].attrs));
    }
}

#[test]
fn collapse_alignments_ignores_quoted_strings() {
    let quoted = "declare void @f() \"align 16 align 32\"";
    assert_eq!(collapse_alignments(quoted.to_string()), quoted);
    assert_eq!(
        collapse_alignments("declare void @f() align 16 align 32".to_string()),
        "declare void @f() align 32"
    );
}

#[test]
fn round_trips_function_local_aggregate_constants() {
    let source = r#"
define i32 @literal_extract() {
entry:
  %x = extractvalue { i32, i32 } { i32 7, i32 8 }, 1
  ret i32 %x
}

define { i32, i32 } @literal_insert() {
entry:
  %x = insertvalue { i32, i32 } { i32 7, i32 8 }, i32 9, 0
  ret { i32, i32 } %x
}
"#;
    let mut module = parse_module(source).expect("parse local aggregate constants");
    let bytes = write_bitcode(&mut module).expect("write local aggregate constants");
    let read = read_bitcode(&bytes).expect("read local aggregate constants");
    let text = print_module(&read);
    assert!(
        text.contains("%x = extractvalue { i32, i32 } { i32 7, i32 8 }, 1"),
        "{text}"
    );
    assert!(
        text.contains("%x = insertvalue { i32, i32 } { i32 7, i32 8 }, i32 9, 0"),
        "{text}"
    );
}

#[test]
fn round_trips_va_arg() {
    let source = r#"
%struct.S = type { i32, i64 }

define void @f(ptr %ap) {
entry:
  %a = va_arg ptr %ap, i32
  %b = va_arg ptr %ap, <4 x i32>
  %c = va_arg ptr %ap, %struct.S
  ret void
}
"#;
    let mut module = parse_module(source).expect("parse va_arg");
    let bytes = write_bitcode(&mut module).expect("write va_arg");
    let read = read_bitcode(&bytes).expect("read va_arg");
    let text = print_module(&read);
    assert!(text.contains("%a = va_arg ptr %ap, i32"), "{text}");
    assert!(text.contains("%b = va_arg ptr %ap, <4 x i32>"), "{text}");
    assert!(text.contains("%c = va_arg ptr %ap, %struct.S"), "{text}");
}

#[test]
fn round_trips_forward_element_operands() {
    let source = r#"
define i32 @f(i32 %n) {
entry:
  br label %def
use:
  %x = extractelement <2 x i32> %v, i32 0
  ret i32 %x
def:
  %v = insertelement <2 x i32> zeroinitializer, i32 %n, i32 0
  ret i32 0
}
"#;
    let mut module = parse_module(source).expect("parse forward element operand");
    let bytes = write_bitcode(&mut module).expect("write forward element operand");
    let read = read_bitcode(&bytes).expect("read forward element operand");
    let text = print_module(&read);
    assert!(
        text.contains("%x = extractelement <2 x i32> %v, i32 0"),
        "{text}"
    );
    assert!(
        text.contains("%v = insertelement <2 x i32> zeroinitializer, i32 %n, i32 0"),
        "{text}"
    );
}

#[test]
fn round_trips_blockaddress_and_indirectbr() {
    let source = r#"
@dispatch = global ptr blockaddress(@f, %case_a)
@dispatch_aligned = global ptr blockaddress(@f, %case_b), align 8

define void @f(i1 %c) {
entry:
  br i1 %c, label %case_a, label %case_b

case_a:
  br label %exit

case_b:
  br label %exit

exit:
  %target = phi ptr [ blockaddress(@f, %case_a), %case_a ], [ blockaddress(@f, %case_b), %case_b ]
  indirectbr ptr %target, [label %case_a, label %case_b]
}

define void @empty(ptr %target) {
entry:
  indirectbr ptr %target, []
}
"#;
    let mut module = parse_module(source).expect("parse blockaddress");
    let bytes = write_bitcode(&mut module).expect("write blockaddress");
    let read = read_bitcode(&bytes).expect("read blockaddress");
    let text = print_module(&read);
    assert!(
        text.contains("@dispatch = global ptr blockaddress(@f, %case_a)"),
        "{text}"
    );
    assert!(
        text.contains("@dispatch_aligned = global ptr blockaddress(@f, %case_b), align 8"),
        "{text}"
    );
    assert!(
        text.contains(
            "%target = phi ptr [ blockaddress(@f, %case_a), %case_a ], [ blockaddress(@f, %case_b), %case_b ]"
        ),
        "{text}"
    );
    assert!(
        text.contains("indirectbr ptr %target, [label %case_a, label %case_b]"),
        "{text}"
    );
    assert!(text.contains("indirectbr ptr %target, []"), "{text}");
}

#[test]
fn round_trips_inline_asm_and_callbr() {
    let source = r#"
define void @jump() {
entry:
  callbr void asm sideeffect "jmp ${0:l}", "!i"() to label %fallthrough [label %indirect]

fallthrough:
  ret void

indirect:
  ret void
}

define i32 @with_result(i32 %x) {
entry:
  %r = callbr i32 asm "add $0, $1", "=r,r,!i"(i32 %x) to label %fallthrough [label %indirect]

fallthrough:
  ret i32 %r

indirect:
  ret i32 0
}

define void @asm_call() {
entry:
  call void asm sideeffect "nop", ""()
  ret void
}
"#;
    let mut module = parse_module(source).expect("parse inline asm");
    let bytes = write_bitcode(&mut module).expect("write inline asm");
    let read = read_bitcode(&bytes).expect("read inline asm");
    let text = print_module(&read);
    assert!(
        text.contains(
            "callbr void asm sideeffect \"jmp ${0:l}\", \"!i\"() to label %fallthrough [label %indirect]"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "%r = callbr i32 asm \"add $0, $1\", \"=r,r,!i\"(i32 %x) to label %fallthrough [label %indirect]"
        ),
        "{text}"
    );
    assert!(
        text.contains("call void asm sideeffect \"nop\", \"\"()"),
        "{text}"
    );
}

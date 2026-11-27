use llvm_rust_ir::{parse_module, print_module, verify_module};

fn roundtrip(source: &str) -> String {
    let module = parse_module(source)
        .unwrap_or_else(|error| panic!("parse failed: {}", error.render("test.ll", source)));
    if let Err(errors) = verify_module(&module) {
        panic!("verification failed: {errors:?}");
    }
    let printed = print_module(&module);
    let reparsed = parse_module(&printed)
        .unwrap_or_else(|error| panic!("reparse failed: {error}\n--- printed ---\n{printed}"));
    if let Err(errors) = verify_module(&reparsed) {
        panic!("re-verification failed: {errors:?}\n--- printed ---\n{printed}");
    }
    let printed_again = print_module(&reparsed);
    assert_eq!(printed, printed_again, "printer is not idempotent");
    printed
}

#[test]
fn simple_add() {
    let source = r#"
define i32 @add(i32 %a, i32 %b) {
entry:
  %sum = add nsw i32 %a, %b
  ret i32 %sum
}
"#;
    let printed = roundtrip(source);
    assert!(printed.contains("define i32 @add(i32 %a, i32 %b)"));
    assert!(printed.contains("%sum = add nsw i32 %a, %b"));
}

#[test]
fn headers_types_and_globals() {
    let source = r#"
source_filename = "t.c"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%struct.pair = type { i32, i8 }

@g = global i32 0, align 4
@ext = external global i32
@s = private constant [4 x i8] c"abc\00", align 1
@pair = global %struct.pair zeroinitializer, align 8
"#;
    let printed = roundtrip(source);
    assert!(printed.contains("%struct.pair = type { i32, i8 }"));
    assert!(printed.contains("@g = global i32 0, align 4"));
    assert!(printed.contains("@ext = external global i32"));
    assert!(printed.contains("@s = private constant [4 x i8] c\"abc\\00\", align 1"));
    assert!(printed.contains("@pair = global %struct.pair zeroinitializer, align 8"));
}

#[test]
fn control_flow_and_calls() {
    let source = r#"
@s = private constant [4 x i8] c"abc\00", align 1
declare i32 @puts(ptr)

define i32 @f(i32 %n) {
entry:
  %p = alloca i32, align 4
  store i32 %n, ptr %p, align 4
  %v = load i32, ptr %p, align 4
  %cmp = icmp sgt i32 %v, 0
  br i1 %cmp, label %then, label %else

then:
  %r = call i32 @puts(ptr @s)
  br label %exit

else:
  %r2 = add i32 %v, 1
  br label %exit

exit:
  %phi = phi i32 [ %r, %then ], [ %r2, %else ]
  ret i32 %phi
}
"#;
    let printed = roundtrip(source);
    assert!(printed.contains("%p = alloca i32, align 4"));
    assert!(printed.contains("store i32 %n, ptr %p, align 4"));
    assert!(printed.contains("%r = call i32 @puts(ptr @s)"));
    assert!(printed.contains("%phi = phi i32 [ %r, %then ], [ %r2, %else ]"));
}

#[test]
fn gep_switch_select_and_casts() {
    let source = r#"
%struct.pair = type { i32, i8 }
@pair = global %struct.pair zeroinitializer, align 8

define i32 @g(i32 %x, ptr %p) {
entry:
  %slot = getelementptr inbounds %struct.pair, ptr %p, i64 0, i32 0
  %v = load i32, ptr %slot, align 4
  %sel = select i1 true, i32 %v, i32 %x
  switch i32 %sel, label %default [ i32 0, label %zero i32 1, label %one ]

zero:
  ret i32 0

one:
  ret i32 1

default:
  %z = zext i32 %sel to i64
  %t = trunc i64 %z to i32
  ret i32 %t
}
"#;
    let printed = roundtrip(source);
    assert!(printed.contains("%slot = getelementptr inbounds %struct.pair, ptr %p, i64 0, i32 0"));
    assert!(printed.contains("%sel = select i1 true, i32 %v, i32 %x"));
    assert!(
        printed
            .contains("switch i32 %sel, label %default [ i32 0, label %zero i32 1, label %one ]")
    );
    assert!(printed.contains("%z = zext i32 %sel to i64"));
}

#[test]
fn floating_point() {
    let source = r#"
define double @fmul(double %a, double %b) {
entry:
  %m = fmul fast double %a, %b
  %n = fneg double %m
  %c = fcmp oeq double %n, 0.0
  %s = select i1 %c, double 1.0, double %n
  ret double %s
}
"#;
    let printed = roundtrip(source);
    assert!(printed.contains("fmul fast double %a, %b"));
    assert!(printed.contains("fcmp oeq double %n, 0x0000000000000000"));
    assert!(printed.contains("select i1 %c, double 0x3FF0000000000000, double %n"));
}

#[test]
fn advanced_ir_features() {
    let source = r#"
%opaque = type opaque
%node = type { ptr, i32 }

@tls = thread_local(localexec) global i32 0, section ".tdata", align 4
@wide = global i128 170141183460469231731687303715884105727
@neg = global i128 -1
@one = global float 0x3FF0000000000000
@op = external global %opaque

declare void @sink(ptr byval(%node) align 8, i32 signext)

define i32 @f(i32 %x) align 16 {
  %1 = add i32 %x, 1
  br label %2

2:
  %3 = phi i32 [ %1, %0 ], [ %3, %2 ]
  %4 = add i32 %3, 1
  %5 = icmp slt i32 %4, 10
  call void @sink(ptr null, i32 signext %4)
  br i1 %5, label %2, label %6

6:
  ret i32 %4
}
"#;
    let printed = roundtrip(source);
    assert!(printed.contains("%opaque = type opaque"));
    assert!(
        printed
            .contains("@tls = thread_local(localexec) global i32 0, align 4, section \".tdata\"")
    );
    assert!(printed.contains("@wide = global i128 170141183460469231731687303715884105727"));
    assert!(printed.contains("@neg = global i128 -1"));
    assert!(printed.contains("@one = global float 0x3FF0000000000000"));
    assert!(printed.contains("declare void @sink(ptr byval(%node) align 8, i32 signext)"));
    assert!(printed.contains("define i32 @f(i32 %x) align 16"));
    assert!(printed.contains("%3 = phi i32 [ %1, %0 ], [ %3, %2 ]"));
    assert!(printed.contains("call void @sink(ptr null, i32 signext %4)"));
}

#[test]
fn aggregate_and_vector_element_ops() {
    let source = r#"
%pair = type { i32, [2 x i8] }

define i32 @extract_pair(%pair %p) {
entry:
  %v = extractvalue %pair %p, 1, 0
  %x = sext i8 %v to i32
  ret i32 %x
}

define %pair @insert_pair(%pair %p, i8 %v) {
entry:
  %q = insertvalue %pair %p, i8 %v, 1, 1
  ret %pair %q
}

define i32 @vector_ops(<4 x i32> %v, i32 %idx) {
entry:
  %e = extractelement <4 x i32> %v, i32 %idx
  %n = insertelement <4 x i32> %v, i32 %e, i32 0
  %r = extractelement <4 x i32> %n, i32 0
  ret i32 %r
}
"#;
    let printed = roundtrip(source);
    assert!(printed.contains("%v = extractvalue %pair %p, 1, 0"));
    assert!(printed.contains("%q = insertvalue %pair %p, i8 %v, 1, 1"));
    assert!(printed.contains("%e = extractelement <4 x i32> %v, i32 %idx"));
    assert!(printed.contains("%n = insertelement <4 x i32> %v, i32 %e, i32 0"));
}

#[test]
fn varargs_and_attribute_groups() {
    let source = r#"
declare i32 @printf(ptr, ...)

attributes #0 = { nounwind "frame-pointer"="all" }

define void @h(ptr %p) #0 {
entry:
  %x = call i32 (ptr, ...) @printf(ptr %p)
  ret void
}
"#;
    let printed = roundtrip(source);
    assert!(printed.contains("declare i32 @printf(ptr, ...)"));
    assert!(printed.contains("attributes #0 = { nounwind \"frame-pointer\"=\"all\" }"));
    assert!(printed.contains("define void @h(ptr %p) #0"));
}

#[test]
fn verifier_rejects_missing_terminator() {
    let source = r#"
define i32 @bad() {
entry:
  %x = add i32 1, 2
}
"#;
    let module = parse_module(source).expect("parse should succeed");
    let errors = verify_module(&module).expect_err("verifier should reject the block");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("terminator"))
    );
}

#[test]
fn parser_rejects_undefined_local() {
    let source = r#"
define i32 @bad() {
entry:
  %x = add i32 %missing, 1
  ret i32 %x
}
"#;
    let error = parse_module(source).expect_err("parse should fail");
    assert!(error.message.contains("undefined local"));
}

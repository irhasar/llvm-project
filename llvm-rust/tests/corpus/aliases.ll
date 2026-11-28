source_filename = "aliases.c"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

@target = global i32 7, align 4
@arr = global [4 x i8] zeroinitializer, align 1
@alias_int = alias i32, ptr @target
@weak_alias = weak alias i32, ptr @target
@hidden_alias = hidden alias i32, ptr @target
@gep_alias = alias i8, getelementptr ([4 x i8], ptr @arr, i64 0, i64 2)
@tls_alias = thread_local(initialexec) alias i32, ptr @target
@alias_to_alias = alias i32, ptr @hidden_alias
@0 = global i32 9
@1 = alias i32, ptr @0

define void @f() {
  ret void
}

@alias_fn = alias void (), ptr @f

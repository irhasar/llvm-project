source_filename = "ifunc.c"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

@g = global i32 1
@foo = ifunc i32 (), ptr @resolver
@bar = dso_local ifunc i64 (i32), ptr @resolver2
@a = alias i32, ptr @g

define ptr @resolver() {
  ret ptr @impl
}

define i32 @impl() {
  ret i32 7
}

define ptr @resolver2() {
  ret ptr @impl2
}

define i64 @impl2(i32 %x) {
  %v = sext i32 %x to i64
  ret i64 %v
}

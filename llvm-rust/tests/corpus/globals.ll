source_filename = "t.c"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%struct.pair = type { i32, i8 }

@g = global i32 0, align 4
@ext = external global i32
@s = private constant [4 x i8] c"abc\00", align 1
@pair = global %struct.pair zeroinitializer, align 8

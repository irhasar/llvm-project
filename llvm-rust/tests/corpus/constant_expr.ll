@a = global i32 0, align 4
@arr = global [4 x i32] zeroinitializer, align 4
@b = global ptr getelementptr (i32, ptr @a, i64 1)
@c = global ptr getelementptr inbounds ([4 x i32], ptr @arr, i64 0, i64 2)
@d = global i64 ptrtoint (ptr @a to i64)
@e = global ptr inttoptr (i64 8 to ptr)
@nested = global i64 ptrtoint (ptr getelementptr (i32, ptr @a, i64 1) to i64)
@nested_gep = global ptr getelementptr inbounds (i32, ptr getelementptr (i32, ptr @a, i64 1), i64 2)

define ptr @f() {
entry:
  ret ptr getelementptr (i32, ptr @a, i64 2)
}

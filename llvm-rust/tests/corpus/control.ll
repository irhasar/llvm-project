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

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

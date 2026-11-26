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

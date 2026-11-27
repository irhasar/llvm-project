%struct.S = type { i32, i64 }

define void @f(ptr %ap) {
entry:
  %a = va_arg ptr %ap, i32
  %b = va_arg ptr %ap, i64
  %c = va_arg ptr %ap, double
  %d = va_arg ptr %ap, ptr
  %e = va_arg ptr %ap, <4 x i32>
  %f = va_arg ptr %ap, %struct.S
  ret void
}

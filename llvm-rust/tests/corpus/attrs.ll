declare i32 @g(i32 noundef)

define noundef i32 @f(i32 noundef signext %x) #0 {
entry:
  %r = call i32 @g(i32 %x)
  ret i32 %r
}

define ptr @h(ptr noundef nonnull align 8 dereferenceable(8) %p) nounwind "frame-pointer"="all" {
entry:
  ret ptr %p
}

define i32 @call_attr(i32 %x) {
entry:
  %r = call i32 @g(i32 %x) #0
  ret i32 %r
}

attributes #0 = { nounwind readonly }

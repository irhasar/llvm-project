declare void @use(ptr align 8, i32 noundef)
@slot = global i32 0

define i32 @f(i32 %a) {
entry:
  %v = add i32 %a, 1
  call void @use(ptr align 8 @slot, i32 noundef %v)
  ret i32 %v
}

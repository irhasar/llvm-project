@"" = global i32 1

@"0" = global i32 2

define i32 @f() {
entry:
  %a = load i32, ptr @0
  %b = load i32, ptr @"0"
  %c = add i32 %a, %b
  ret i32 %c
}

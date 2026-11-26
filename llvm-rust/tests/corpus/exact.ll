define i32 @shr(i32 %x, i32 %y) {
entry:
  %a = lshr exact i32 %x, %y
  %b = ashr exact i32 %a, %y
  ret i32 %b
}

define i32 @div(i32 %x, i32 %y) {
entry:
  %a = udiv exact i32 %x, %y
  %b = sdiv exact i32 %a, %y
  ret i32 %b
}

%pair = type { i32, [2 x i8] }

define i32 @extract_pair(%pair %p) {
entry:
  %v = extractvalue %pair %p, 1, 0
  %x = sext i8 %v to i32
  ret i32 %x
}

define %pair @insert_pair(%pair %p, i8 %v) {
entry:
  %q = insertvalue %pair %p, i8 %v, 1, 1
  ret %pair %q
}

define i32 @vector_ops(<4 x i32> %v, i32 %idx) {
entry:
  %e = extractelement <4 x i32> %v, i32 %idx
  %n = insertelement <4 x i32> %v, i32 %e, i32 0
  %r = extractelement <4 x i32> %n, i32 0
  ret i32 %r
}

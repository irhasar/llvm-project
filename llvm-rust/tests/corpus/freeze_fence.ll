define i32 @freeze_int(i32 %x) {
entry:
  %f = freeze i32 %x
  ret i32 %f
}

define <4 x i32> @freeze_vector(<4 x i32> %v) {
entry:
  %f = freeze <4 x i32> %v
  ret <4 x i32> %f
}

define double @freeze_double(double %x) {
entry:
  %f = freeze double %x
  ret double %f
}

define void @fences() {
entry:
  fence acquire
  fence release
  fence acq_rel
  fence seq_cst
  fence syncscope("singlethread") seq_cst
  ret void
}

define <4 x i32> @vector_int(<4 x i32> %a, <4 x i32> %b) {
entry:
  %sum = add <4 x i32> %a, %b
  %prod = mul <4 x i32> %sum, %b
  %bits = and <4 x i32> %prod, %a
  %cmp = icmp slt <4 x i32> %bits, %a
  %sel = select <4 x i1> %cmp, <4 x i32> %bits, <4 x i32> %a
  ret <4 x i32> %sel
}

define <2 x double> @vector_float(<2 x double> %a, <2 x double> %b) {
entry:
  %sum = fadd <2 x double> %a, %b
  %prod = fmul fast <2 x double> %sum, %b
  %cmp = fcmp olt <2 x double> %prod, %a
  %sel = select <2 x i1> %cmp, <2 x double> %prod, <2 x double> %a
  ret <2 x double> %sel
}

define <4 x i32> @vector_shuffle(<4 x i32> %a, <4 x i32> %b) {
entry:
  %r = shufflevector <4 x i32> %a, <4 x i32> %b, <4 x i32> <i32 0, i32 5, i32 2, i32 7>
  ret <4 x i32> %r
}

define <2 x i32> @vector_shrink(<4 x i32> %a, <4 x i32> %b) {
entry:
  %r = shufflevector <4 x i32> %a, <4 x i32> %b, <2 x i32> <i32 1, i32 6>
  ret <2 x i32> %r
}

define <4 x i32> @vector_undef_mask(<4 x i32> %a, <4 x i32> %b) {
entry:
  %r = shufflevector <4 x i32> %a, <4 x i32> %b, <4 x i32> undef
  ret <4 x i32> %r
}

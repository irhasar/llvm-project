define i64 @arith(i64 %a, i64 %b) {
entry:
  %neg = sub i64 0, %a
  %ud = udiv exact i64 %a, %b
  %sh = shl i64 %ud, 3
  %ls = lshr i64 %sh, 1
  %as = ashr i64 %neg, 2
  %an = and i64 %as, 255
  %or = or i64 %an, %ls
  %xo = xor i64 %or, -1
  %tr = trunc i64 %xo to i32
  %ze = zext i32 %tr to i64
  %se = sext i32 %tr to i64
  ret i64 %se
}

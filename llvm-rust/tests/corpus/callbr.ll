; Inline asm constants and callbr terminators.

define void @jump() {
entry:
  callbr void asm sideeffect "jmp ${0:l}", "!i"() to label %fallthrough [label %indirect]

fallthrough:
  ret void

indirect:
  ret void
}

define i32 @with_result(i32 %x) {
entry:
  %r = callbr i32 asm "add $0, $1", "=r,r,!i"(i32 %x) to label %fallthrough [label %indirect]

fallthrough:
  ret i32 %r

indirect:
  ret i32 0
}

define void @asm_call() {
entry:
  call void asm sideeffect "nop", ""()
  ret void
}

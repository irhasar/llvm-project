target triple = "x86_64-unknown-linux-gnu"

define i32 @rmw_int(ptr %p, i32 %v) {
entry:
  %a = atomicrmw add ptr %p, i32 %v seq_cst, align 4
  %b = atomicrmw volatile xchg ptr %p, i32 %v syncscope("singlethread") acquire, align 4
  %c = atomicrmw uinc_wrap ptr %p, i32 %v monotonic, align 4
  %d = atomicrmw nand ptr %p, i32 %v release, align 4
  ret i32 %a
}

define float @rmw_float(ptr %p, float %v) {
entry:
  %a = atomicrmw fadd ptr %p, float %v seq_cst, align 4
  %b = atomicrmw fmin ptr %p, float %v acquire, align 4
  ret float %a
}

define {i32, i1} @cmpxchg_i32(ptr %p, i32 %c, i32 %n) {
entry:
  %r = cmpxchg ptr %p, i32 %c, i32 %n seq_cst seq_cst, align 4
  %r2 = cmpxchg weak volatile ptr %p, i32 %c, i32 %n acquire monotonic, align 4
  ret {i32, i1} %r
}

define {i64, i1} @cmpxchg_i64(ptr %p, i64 %c, i64 %n) {
entry:
  %r = cmpxchg ptr %p, i64 %c, i64 %n syncscope("singlethread") acq_rel acquire, align 8
  ret {i64, i1} %r
}

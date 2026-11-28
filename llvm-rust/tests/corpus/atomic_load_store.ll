define i32 @load_seq_cst(ptr %p) {
entry:
  %v = load atomic i32, ptr %p seq_cst, align 4
  ret i32 %v
}

define void @store_release(ptr %p, i32 %v) {
entry:
  store atomic i32 %v, ptr %p release, align 4
  ret void
}

define i32 @load_single(ptr %p) {
entry:
  %v = load atomic i32, ptr %p syncscope("singlethread") acquire, align 4
  ret i32 %v
}

define void @store_single(ptr %p, i32 %v) {
entry:
  store atomic i32 %v, ptr %p syncscope("singlethread") monotonic, align 4
  ret void
}

define i32 @load_volatile(ptr %p) {
entry:
  %v = load atomic volatile i32, ptr %p unordered, align 4
  ret i32 %v
}

define void @store_volatile(ptr %p, i32 %v) {
entry:
  store atomic volatile i32 %v, ptr %p seq_cst, align 4
  ret void
}

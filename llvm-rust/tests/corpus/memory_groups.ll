declare void @callee()
declare void @inline_legacy(ptr) readnone #0
declare void @inline_modern(ptr) memory(argmem: write) #1

define void @caller() {
entry:
  call void @callee() readnone #0
  ret void
}

attributes #0 = { memory(argmem: write) }
attributes #1 = { readnone }

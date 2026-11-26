declare void @f(ptr readonly, ptr readnone, ptr writeonly)

declare void @g(ptr readonly) memory(read)

define void @h(ptr readonly %p) {
entry:
  call void @g(ptr readonly %p)
  ret void
}

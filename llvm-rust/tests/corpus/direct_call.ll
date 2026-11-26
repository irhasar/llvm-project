declare void @callee()

define void @caller() {
entry:
  call void @callee()
  ret void
}

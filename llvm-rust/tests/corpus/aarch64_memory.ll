target triple = "aarch64-unknown-linux-gnu"

declare void @callee()
declare void @read_default_write_inaccessible() memory(read, inaccessiblemem: write)

define void @caller() {
entry:
  call void @callee() memory(read, inaccessiblemem: write)
  ret void
}

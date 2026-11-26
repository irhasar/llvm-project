target triple = "aarch64-unknown-linux-gnu"

declare void @read_target() memory(inaccessiblemem: read, target_mem: read)
declare void @write_target() memory(write, inaccessiblemem: write, errnomem: write)
declare void @read_default_write_inaccessible() memory(read, inaccessiblemem: write, target_mem: write)
declare void @write_default_no_inaccessible() memory(write, inaccessiblemem: none, target_mem: none)
declare void @callee()

define void @caller() {
entry:
  call void @callee() memory(read, inaccessiblemem: write, target_mem: write)
  ret void
}

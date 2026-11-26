target triple = "aarch64-unknown-linux-gnu"

declare void @read_target() memory(inaccessiblemem: read, target_mem0: read, target_mem1: read)
declare void @write_target() memory(write, inaccessiblemem: write, errnomem: write)

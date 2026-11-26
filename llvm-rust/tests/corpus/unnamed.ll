@lg = local_unnamed_addr global i32 0
@ug = unnamed_addr global i32 1

define void @lf() local_unnamed_addr {
entry:
  ret void
}

declare void @uf() unnamed_addr

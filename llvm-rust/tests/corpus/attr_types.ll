%"a b" = type { i64 }
%ab = type { i8 }

declare void @take(ptr byval(%"a b"), ptr byval([8 x i8]))
declare void @other(ptr byval(%ab))
declare void @aligned() alignstack(16)
declare void @grouped() #0

define void @mem() #0 {
entry:
  ret void
}

attributes #0 = { memory(none) alignstack=16 }

define void @falign() align 16 {
entry:
  ret void
}

declare void @fgroup() #1

attributes #1 = { align=16 }

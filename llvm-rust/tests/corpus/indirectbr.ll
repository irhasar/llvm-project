@dispatch = global ptr blockaddress(@dispatch_fn, %case_a)
@dispatch_aligned = global ptr blockaddress(@dispatch_fn, %case_b), align 8
@unnamed_block = global ptr blockaddress(@unnamed, %1)

define void @dispatch_fn(i1 %c) {
entry:
  br i1 %c, label %case_a, label %case_b

case_a:
  br label %exit

case_b:
  br label %exit

exit:
  %target = phi ptr [ blockaddress(@dispatch_fn, %case_a), %case_a ], [ blockaddress(@dispatch_fn, %case_b), %case_b ]
  indirectbr ptr %target, [label %case_a, label %case_b]
}

define ptr @dispatch_target(i1 %c) {
entry:
  br i1 %c, label %a, label %b

a:
  ret ptr blockaddress(@dispatch_fn, %case_a)

b:
  ret ptr blockaddress(@dispatch_fn, %case_b)
}

define void @unnamed(ptr %target) {
0:
  indirectbr ptr %target, [label %1, label %2]

1:
  ret void

2:
  ret void
}

define void @no_destinations(ptr %target) {
entry:
  indirectbr ptr %target, []
}

define void @duplicate_destinations(ptr %target) {
entry:
  indirectbr ptr %target, [label %a, label %a]

a:
  ret void
}

define void @direct_blockaddress() {
entry:
  indirectbr ptr blockaddress(@direct_blockaddress, %a), [label %a]

a:
  ret void
}

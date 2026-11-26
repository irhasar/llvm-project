define i32 @loop(i32 %n) {
entry:
  br label %loop

loop:
  %i = phi i32 [ 0, %entry ], [ %next, %loop ]
  %next = add i32 %i, 1
  %done = icmp slt i32 %next, %n
  br i1 %done, label %loop, label %exit

exit:
  ret i32 %next
}

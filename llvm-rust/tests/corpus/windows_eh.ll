declare void @may_throw()
declare i32 @__CxxFrameHandler3(...)
declare void @dtor(ptr)

define void @f() personality ptr @__CxxFrameHandler3 {
entry:
  invoke void @may_throw() to label %cont unwind label %dispatch

dispatch:
  %cs = catchswitch within none [label %catch1, label %catch2] unwind label %cleanup

catch1:
  %cp1 = catchpad within %cs [ptr null, i32 64, ptr null]
  catchret from %cp1 to label %cont

catch2:
  %cp2 = catchpad within %cs [ptr null, i32 64, ptr null]
  catchret from %cp2 to label %cont

cleanup:
  %lp = cleanuppad within none [ptr @dtor]
  cleanupret from %lp unwind to caller

cont:
  ret void
}

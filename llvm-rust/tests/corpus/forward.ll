@g = global ptr @later
@fp = global ptr @f
@later = global i32 7

define ptr @ret_later() {
entry:
  ret ptr @later
}

define ptr @ret_fn() {
entry:
  ret ptr @f
}

define void @f() {
entry:
  ret void
}

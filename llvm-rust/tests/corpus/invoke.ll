declare i32 @may_throw()
declare i32 @__gxx_personality_v0(...)

define i32 @f() personality ptr @__gxx_personality_v0 {
entry:
  %r = invoke i32 @may_throw() to label %normal unwind label %lpad
normal:
  ret i32 %r
lpad:
  %lp = landingpad { ptr, i32 } cleanup catch ptr null filter [0 x ptr] zeroinitializer
  resume { ptr, i32 } %lp
}

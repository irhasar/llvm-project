define double @fmul(double %a, double %b) {
entry:
  %m = fmul fast double %a, %b
  %n = fneg double %m
  %c = fcmp oeq double %n, 0.0
  %s = select i1 %c, double 1.0, double %n
  ret double %s
}

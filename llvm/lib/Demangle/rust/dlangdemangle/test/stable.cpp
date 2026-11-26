//===-- stable.cpp - Golden vectors for llvm::dlangDemangle --------------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
// Runs against whichever backend LLVMDemangle was configured with, so the same
// expected values prove that the Rust and C++ implementations agree.
//
//===----------------------------------------------------------------------===//

#include "llvm/Demangle/Demangle.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string_view>

static unsigned Failures;

static void check(std::string_view Mangled, const char *Expected) {
  char *Demangled = llvm::dlangDemangle(Mangled);
  const char *Actual = Demangled ? Demangled : "<null>";
  if ((Expected == nullptr) != (Demangled == nullptr) ||
      (Expected && std::strcmp(Expected, Demangled) != 0)) {
    std::fprintf(stderr, "FAIL %.*s\n  expected: %s\n  actual:   %s\n",
                 static_cast<int>(Mangled.size()), Mangled.data(),
                 Expected ? Expected : "<null>", Actual);
    ++Failures;
  }
  std::free(Demangled);
}

int main() {
  check("_Dmain", "D main");
  check("_D3fooi", "int foo");
  check("_D3foo3bari", "int foo.bar");
  check("_D3foo3barZ", "foo.bar");
  check("_D3foo6__initZ", "initializer for foo");
  check("_D3foo6__vtblZ", "vtable for foo");
  check("_D3foo7__ClassZ", "ClassInfo for foo");
  check("_D3foo11__InterfaceZ", "Interface for foo");
  check("_D3foo12__ModuleInfoZ", "ModuleInfo for foo");
  check("_D3foo3barNn", "noreturn foo.bar");
  check("_D3foo3barzi", "cent foo.bar");
  check("_D3foo3barzk", "ucent foo.bar");
  check("_D3foo3barQeZ", "foo.bar.bar");
  check("_D3foo3barQe3bazZ", "foo.bar.bar.baz");
  check("_D3foo3barQeQe", "char foo.bar.bar");
  check("_D3foo3barQe3bazQeZ", "foo.bar.bar.baz.baz");

  check("", nullptr);
  check("_D", nullptr);
  check("_D3foo", nullptr);
  check("_D3foobar", nullptr);
  check("_D3foo3barQa", nullptr);
  check("_D3foo3barz", nullptr);
  check("_D3foo3barX", nullptr);
  check("_Z3foov", nullptr);

  if (Failures != 0) {
    std::fprintf(stderr, "%u failures\n", Failures);
    return 1;
  }
  std::printf("OK: llvm::dlangDemangle golden vectors\n");
  return 0;
}

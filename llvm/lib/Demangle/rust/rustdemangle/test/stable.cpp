//===-- stable.cpp - Golden vectors for llvm::rustDemangle ----------------===//
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
  char *Demangled = llvm::rustDemangle(Mangled);
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
  check("_RNvC6_123foo3bar", "123foo::bar");
  check("_RNvNtNtCs8uuN1nrGffB_4core3str5count14do_count_chars",
        "core::str::count::do_count_chars");
  check("_RINvMNtCs8uuN1nrGffB_4core3stre12trim_matchesNvMNtNtB5_4char7methods"
        "c13is_whitespaceECsh0g7Hz38UNf_3std",
        "<str>::trim_matches::<<char>::is_whitespace>");
  check("_RNvXsR_NtCs8uuN1nrGffB_4core6optionINtB5_6OptionmENtNtB7_3fmt5Debug"
        "3fmtCsh0g7Hz38UNf_3std",
        "<core::option::Option<u32> as core::fmt::Debug>::fmt");
  check("_RCu9_bcher_kva", "bücher");
  check("_RNvC6_123foo3bar.llvm.1234", "123foo::bar (.llvm.1234)");

  check("", nullptr);
  check("_Z3foov", nullptr);
  check("_R", nullptr);

  if (Failures != 0) {
    std::fprintf(stderr, "%u failures\n", Failures);
    return 1;
  }
  std::printf("OK: llvm::rustDemangle golden vectors\n");
  return 0;
}

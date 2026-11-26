//===-- stable.cpp - StringExtras golden vectors -----------------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/ADT/StringExtras.h"

#include <cstdio>
#include <string>

using namespace llvm;

namespace {

bool checkSnake(StringRef Input, StringRef Expected) {
  std::string Actual = convertToSnakeFromCamelCase(Input);
  if (Actual != Expected) {
    std::fprintf(stderr, "snake mismatch for '%.*s': got '%.*s'\n",
                 static_cast<int>(Input.size()), Input.data(),
                 static_cast<int>(Actual.size()), Actual.data());
    return false;
  }
  return true;
}

bool checkCamel(StringRef Input, bool CapitalizeFirst, StringRef Expected) {
  std::string Actual = convertToCamelFromSnakeCase(Input, CapitalizeFirst);
  if (Actual != Expected) {
    std::fprintf(stderr, "camel mismatch for '%.*s': got '%.*s'\n",
                 static_cast<int>(Input.size()), Input.data(),
                 static_cast<int>(Actual.size()), Actual.data());
    return false;
  }
  return true;
}

} // namespace

int main() {
  if (!checkSnake("", "") || !checkSnake("A", "a") ||
      !checkSnake("aB", "a_b") || !checkSnake("HTTPServer", "http_server") ||
      !checkSnake("ABCDef", "abc_def") ||
      !checkSnake("foo123Bar", "foo123_bar") ||
      !checkSnake("already_snake", "already_snake") ||
      !checkSnake("XMLHTTPRequest", "xmlhttp_request") ||
      !checkCamel("", false, "") || !checkCamel("a", true, "A") ||
      !checkCamel("http_server", false, "httpServer") ||
      !checkCamel("http_server", true, "HttpServer") ||
      !checkCamel("a__b", false, "a_B") || !checkCamel("a_", false, "a_") ||
      !checkCamel("_foo", false, "_foo") ||
      !checkCamel("foo_bar_baz", true, "FooBarBaz"))
    return 1;

  std::printf("OK: StringExtras golden vectors\n");
}

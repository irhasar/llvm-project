//===-- reference.cpp - C++ StringExtras reference ---------------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/ADT/StringExtras.h"
#include "llvm/ADT/STLExtras.h"

#include <cctype>
#include <string>

using namespace llvm;

namespace stringextras_test {

std::string referenceSnake(StringRef input) {
  if (input.empty())
    return "";

  std::string snakeCase;
  snakeCase.reserve(input.size());
  auto check = [&input](size_t j, function_ref<bool(int)> predicate) {
    return j < input.size() && predicate(input[j]);
  };
  for (size_t i = 0; i < input.size(); ++i) {
    snakeCase.push_back(tolower(input[i]));
    if (check(i, isupper) && check(i + 1, isupper) && check(i + 2, islower))
      snakeCase.push_back('_');
    if ((check(i, islower) || check(i, isdigit)) && check(i + 1, isupper))
      snakeCase.push_back('_');
  }
  return snakeCase;
}

std::string referenceCamel(StringRef input, bool capitalizeFirst) {
  if (input.empty())
    return "";

  std::string output;
  output.reserve(input.size());

  if (capitalizeFirst && std::islower(input.front()))
    output.push_back(toUpper(input.front()));
  else
    output.push_back(input.front());

  for (size_t pos = 1, e = input.size(); pos < e; ++pos) {
    if (input[pos] == '_' && pos != (e - 1) && std::islower(input[pos + 1]))
      output.push_back(toUpper(input[++pos]));
    else
      output.push_back(input[pos]);
  }
  return output;
}

} // namespace stringextras_test

//===-- reference.cpp - C++ StringExtras reference ---------------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/ADT/StringExtras.h"
#include "llvm/ADT/STLExtras.h"

#include <string>

using namespace llvm;

namespace stringextras_test {

std::string referenceSnake(StringRef input) {
  if (input.empty())
    return "";

  std::string snakeCase;
  snakeCase.reserve(input.size());
  // LLVM's C-locale helpers are defined for every char value; <cctype> is not
  // defined for negative values other than EOF on signed-char targets.
  auto check = [&input](size_t j, function_ref<bool(char)> predicate) {
    return j < input.size() && predicate(input[j]);
  };
  for (size_t i = 0; i < input.size(); ++i) {
    snakeCase.push_back(toLower(input[i]));
    if (check(i, isUpper) && check(i + 1, isUpper) && check(i + 2, isLower))
      snakeCase.push_back('_');
    if ((check(i, isLower) || check(i, isDigit)) && check(i + 1, isUpper))
      snakeCase.push_back('_');
  }
  return snakeCase;
}

std::string referenceCamel(StringRef input, bool capitalizeFirst) {
  if (input.empty())
    return "";

  std::string output;
  output.reserve(input.size());

  if (capitalizeFirst && isLower(input.front()))
    output.push_back(toUpper(input.front()));
  else
    output.push_back(input.front());

  for (size_t pos = 1, e = input.size(); pos < e; ++pos) {
    if (input[pos] == '_' && pos != (e - 1) && isLower(input[pos + 1]))
      output.push_back(toUpper(input[++pos]));
    else
      output.push_back(input[pos]);
  }
  return output;
}

} // namespace stringextras_test

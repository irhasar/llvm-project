//===-- stable.cpp - NativeFormatting public-API golden test --------------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/Support/NativeFormatting.h"
#include "llvm/Support/raw_ostream.h"

#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>

namespace {

template <typename T>
std::string formatInteger(T Value, size_t MinDigits, llvm::IntegerStyle Style,
                          bool NonNegativePlus = false) {
  std::string Out;
  llvm::raw_string_ostream OS(Out);
  llvm::write_integer(OS, Value, MinDigits, Style, NonNegativePlus);
  return OS.str();
}

std::string formatHex(uint64_t Value, llvm::HexPrintStyle Style,
                      std::optional<size_t> Width = std::nullopt) {
  std::string Out;
  llvm::raw_string_ostream OS(Out);
  llvm::write_hex(OS, Value, Style, Width);
  return OS.str();
}

std::string formatDouble(double Value, llvm::FloatStyle Style,
                         std::optional<size_t> Precision = std::nullopt) {
  std::string Out;
  llvm::raw_string_ostream OS(Out);
  llvm::write_double(OS, Value, Style, Precision);
  return OS.str();
}

bool expect(const char *What, const std::string &Actual,
            const std::string &Expected) {
  if (Actual == Expected)
    return true;
  std::fprintf(stderr, "%s: got '%s', expected '%s'\n", What, Actual.c_str(),
               Expected.c_str());
  return false;
}

uint64_t nextRandom(uint64_t &State) {
  State ^= State << 13;
  State ^= State >> 7;
  State ^= State << 17;
  return State;
}

void benchOne(const char *Name, uint64_t (&Values)[256],
              llvm::raw_null_ostream &OS, void (*Fn)(llvm::raw_ostream &,
                                                      uint64_t)) {
  constexpr unsigned Iterations = 4000000;
  auto Start = std::chrono::steady_clock::now();
  for (unsigned I = 0; I < Iterations; ++I)
    Fn(OS, Values[I & 255]);
  auto End = std::chrono::steady_clock::now();
  double Ns = std::chrono::duration<double, std::nano>(End - Start).count();
  std::printf("%-12s %8.2f ns/op %8.2f Mop/s\n", Name, Ns / Iterations,
              Iterations / Ns * 1000.0);
}

int bench() {
  uint64_t Values[256];
  uint64_t State = 0x9e3779b97f4a7c15ull;
  for (uint64_t &Value : Values)
    Value = nextRandom(State);

  llvm::raw_null_ostream OS;
  benchOne("integer", Values, OS, [](llvm::raw_ostream &S, uint64_t V) {
    llvm::write_integer(S, V, 0, llvm::IntegerStyle::Integer);
  });
  benchOne("grouped", Values, OS, [](llvm::raw_ostream &S, uint64_t V) {
    llvm::write_integer(S, V, 0, llvm::IntegerStyle::Number);
  });
  benchOne("hex", Values, OS, [](llvm::raw_ostream &S, uint64_t V) {
    llvm::write_hex(S, V, llvm::HexPrintStyle::Lower, 16);
  });
  return 0;
}

} // namespace

int main(int argc, char **argv) {
  if (argc == 2 && std::strcmp(argv[1], "--bench") == 0)
    return bench();

  bool Ok = true;

  Ok &= expect("u64 zero", formatInteger<uint64_t>(0, 0, llvm::IntegerStyle::Integer),
               "0");
  Ok &= expect("u64 min digits",
               formatInteger<uint64_t>(42, 5, llvm::IntegerStyle::Integer),
               "00042");
  Ok &= expect("u64 max",
               formatInteger<uint64_t>(UINT64_MAX, 0,
                                       llvm::IntegerStyle::Integer),
               "18446744073709551615");
  Ok &= expect("u32 max",
               formatInteger<uint32_t>(UINT32_MAX, 0,
                                       llvm::IntegerStyle::Integer),
               "4294967295");
  Ok &= expect("i64 min",
               formatInteger<int64_t>(INT64_MIN, 0,
                                      llvm::IntegerStyle::Integer),
               "-9223372036854775808");
  Ok &= expect("negative min digits",
               formatInteger<int64_t>(-1234, 6, llvm::IntegerStyle::Integer),
               "-001234");
  Ok &= expect("non-negative plus",
               formatInteger<int64_t>(1234, 6, llvm::IntegerStyle::Integer,
                                      true),
               "+001234");
  Ok &= expect("grouping",
               formatInteger<uint64_t>(1234567, 0,
                                       llvm::IntegerStyle::Number),
               "1,234,567");
  Ok &= expect("grouping max",
               formatInteger<uint64_t>(UINT64_MAX, 0,
                                       llvm::IntegerStyle::Number),
               "18,446,744,073,709,551,615");
  Ok &= expect("grouping ignores min digits",
               formatInteger<uint64_t>(1234, 10, llvm::IntegerStyle::Number),
               "1,234");

  Ok &= expect("hex zero", formatHex(0, llvm::HexPrintStyle::Lower), "0");
  Ok &= expect("hex prefix zero",
               formatHex(0, llvm::HexPrintStyle::PrefixLower), "0x0");
  Ok &= expect("hex upper", formatHex(0xf, llvm::HexPrintStyle::Upper), "F");
  Ok &= expect("hex width",
               formatHex(0x1234, llvm::HexPrintStyle::Lower, 8), "00001234");
  Ok &= expect("hex prefix width",
               formatHex(0x1234, llvm::HexPrintStyle::PrefixUpper, 8),
               "0x001234");
  Ok &= expect("hex max",
               formatHex(UINT64_MAX, llvm::HexPrintStyle::PrefixLower),
               "0xffffffffffffffff");

  std::string Wide = formatHex(1, llvm::HexPrintStyle::Lower, 200);
  Ok &= expect("hex width clamp", std::to_string(Wide.size()), "128");
  if (Wide.size() == 128)
    Ok &= expect("hex width clamp value", Wide.substr(120), "00000001");

  Ok &= expect("double fixed",
               formatDouble(1.5, llvm::FloatStyle::Fixed, 2), "1.50");
  Ok &= expect("double exponent",
               formatDouble(1.5, llvm::FloatStyle::Exponent, 3), "1.500e+00");

  if (!Ok)
    return 1;

  std::puts("nativeformatting stable: all golden checks passed");
  return 0;
}

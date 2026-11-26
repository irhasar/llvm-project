//===-- differential.cpp - NativeFormatting Rust/C++ differential test ----===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>

extern "C" size_t llvm_nativeformatting_format_decimal(uint64_t Value,
                                                       bool Grouping,
                                                       uint8_t *Out,
                                                       size_t Cap);
extern "C" size_t llvm_nativeformatting_format_hex(uint64_t Value, bool Upper,
                                                   bool Prefix, size_t Width,
                                                   uint8_t *Out, size_t Cap);

namespace {

std::string refDecimal(uint64_t Value, bool Grouping) {
  std::string Digits = std::to_string(Value);
  if (!Grouping || Digits.size() <= 3)
    return Digits;

  std::string Out;
  size_t Initial = (Digits.size() - 1) % 3 + 1;
  Out.append(Digits, 0, Initial);
  for (size_t I = Initial; I < Digits.size(); I += 3) {
    Out.push_back(',');
    Out.append(Digits, I, 3);
  }
  return Out;
}

std::string refHex(uint64_t Value, bool Upper, bool Prefix, size_t Width) {
  Width = std::min<size_t>(128, Width);
  unsigned Nibbles =
      Value == 0 ? 0 : (64 - __builtin_clzll(Value) + 3) / 4;
  size_t PrefixChars = Prefix ? 2 : 0;
  size_t NumChars = std::max(Width, std::max<size_t>(1, Nibbles) + PrefixChars);
  std::string Out(NumChars, '0');
  if (Prefix)
    Out[1] = 'x';

  size_t Pos = NumChars;
  while (Value) {
    unsigned Digit = static_cast<unsigned>(Value & 0xf);
    Out[--Pos] = Digit < 10 ? char('0' + Digit)
                            : char((Upper ? 'A' : 'a') + Digit - 10);
    Value >>= 4;
  }
  return Out;
}

bool checkDecimal(uint64_t Value, bool Grouping, std::string &Error) {
  uint8_t Buffer[256];
  size_t Len = llvm_nativeformatting_format_decimal(
      Value, Grouping, Buffer, sizeof(Buffer));
  std::string Actual(reinterpret_cast<char *>(Buffer), Len);
  std::string Expected = refDecimal(Value, Grouping);
  if (Actual == Expected)
    return true;

  char Msg[256];
  std::snprintf(Msg, sizeof(Msg),
                "decimal mismatch value=%llu grouping=%d rust='%s' ref='%s'",
                static_cast<unsigned long long>(Value), Grouping,
                Actual.c_str(), Expected.c_str());
  Error = Msg;
  return false;
}

bool checkHex(uint64_t Value, bool Upper, bool Prefix, size_t Width,
              std::string &Error) {
  uint8_t Buffer[256];
  size_t Len = llvm_nativeformatting_format_hex(Value, Upper, Prefix, Width,
                                                Buffer, sizeof(Buffer));
  std::string Actual(reinterpret_cast<char *>(Buffer), Len);
  std::string Expected = refHex(Value, Upper, Prefix, Width);
  if (Actual == Expected)
    return true;

  char Msg[320];
  std::snprintf(Msg, sizeof(Msg),
                "hex mismatch value=%llu upper=%d prefix=%d width=%zu "
                "rust='%s' ref='%s'",
                static_cast<unsigned long long>(Value), Upper, Prefix, Width,
                Actual.c_str(), Expected.c_str());
  Error = Msg;
  return false;
}

uint64_t nextRandom(uint64_t &State) {
  State ^= State << 13;
  State ^= State >> 7;
  State ^= State << 17;
  return State;
}

} // namespace

int main() {
  std::string Error;
  const uint64_t Edges[] = {
      0,
      1,
      9,
      10,
      99,
      100,
      999,
      1000,
      999999,
      1000000,
      0xffffffffull,
      0x100000000ull,
      0x7fffffffffffffffull,
      0x8000000000000000ull,
      0xffffffffffffffffull,
  };
  for (uint64_t Value : Edges) {
    for (bool Grouping : {false, true}) {
      if (!checkDecimal(Value, Grouping, Error)) {
        std::fprintf(stderr, "%s\n", Error.c_str());
        return 1;
      }
    }
    for (bool Upper : {false, true}) {
      for (bool Prefix : {false, true}) {
        for (size_t Width : {size_t(0), size_t(1), size_t(4), size_t(8),
                             size_t(16), size_t(128), size_t(200)}) {
          if (!checkHex(Value, Upper, Prefix, Width, Error)) {
            std::fprintf(stderr, "%s\n", Error.c_str());
            return 1;
          }
        }
      }
    }
  }

  uint64_t State = 0x9e3779b97f4a7c15ull;
  for (unsigned I = 0; I < 200000; ++I) {
    uint64_t Value = nextRandom(State);
    if (!checkDecimal(Value, (I & 1) != 0, Error) ||
        !checkDecimal(Value >> (I & 31), (I & 2) != 0, Error) ||
        !checkHex(Value, (I & 1) != 0, (I & 2) != 0, I % 20, Error)) {
      std::fprintf(stderr, "%s\n", Error.c_str());
      return 1;
    }
  }

  // A too-small buffer must report the required size without writing.
  uint8_t Small[2] = {0xaa, 0xbb};
  size_t Required = llvm_nativeformatting_format_decimal(123456, false, Small,
                                                         sizeof(Small));
  if (Required != 6 || Small[0] != 0xaa || Small[1] != 0xbb) {
    std::fprintf(stderr, "short-buffer contract failed\n");
    return 1;
  }

  std::puts("nativeformatting differential: 0 mismatches");
  return 0;
}

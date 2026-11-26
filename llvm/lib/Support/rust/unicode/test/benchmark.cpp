//===-- benchmark.cpp - Unicode fold throughput -----------------*- C++ -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#include "llvm/Support/Unicode.h"
#include <chrono>
#include <cstdio>
#include <random>

int main() {
  constexpr int N = 5000000;
  std::mt19937 RNG(12345);
  std::uniform_int_distribution<int> Dist(0, 0x10FFFF);
  int *Values = new int[N];
  for (int I = 0; I < N; ++I)
    Values[I] = Dist(RNG);

  volatile int Sink = 0;
  auto Start = std::chrono::steady_clock::now();
  for (int I = 0; I < N; ++I)
    Sink += llvm::sys::unicode::foldCharSimple(Values[I]);
  auto End = std::chrono::steady_clock::now();
  double NsPerOp =
      std::chrono::duration<double, std::nano>(End - Start).count() / N;
  std::printf("%.3f ns/op (sink=%d)\n", NsPerOp, Sink);
  delete[] Values;
  return 0;
}

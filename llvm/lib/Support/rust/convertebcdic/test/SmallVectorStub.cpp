//===-- SmallVectorStub.cpp - Standalone SmallVector grow stub -----------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
// The standalone EBCDIC tests keep all outputs inside the inline capacity and
// do not link LLVMSupport. Provide the one out-of-line SmallVector symbol the
// wrapper references; any actual growth aborts the test.
//
//===----------------------------------------------------------------------===//

#include <cstdlib>

extern "C" void smallVectorGrowPodStub(void *, size_t, size_t)
    asm("_ZN4llvm15SmallVectorBaseImE8grow_podEPvmm");

extern "C" void smallVectorGrowPodStub(void *, size_t, size_t) {
  std::abort();
}

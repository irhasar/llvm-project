//===-- SmallVectorStub.cpp - Standalone SmallVector grow stub -----------===//
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

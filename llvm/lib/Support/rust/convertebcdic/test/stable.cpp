//===-- stable.cpp - Golden tests for EBCDIC conversion ------------------===//
//
// Exercises the public ConverterEBCDIC API. Built with the Rust backend when
// LLVM_ENABLE_RUST_CONVERTEBCDIC is enabled and with the C++ backend otherwise.
//
//===----------------------------------------------------------------------===//

#include "llvm/ADT/SmallVector.h"
#include "llvm/ADT/StringRef.h"
#include "llvm/Support/ConvertEBCDIC.h"

#include <cstdio>
#include <cstring>
#include <string>
#include <system_error>

using namespace llvm;

namespace {

int Failures = 0;

void expect(bool Cond, const char *What) {
  if (!Cond) {
    std::fprintf(stderr, "FAIL: %s\n", What);
    ++Failures;
  }
}

void checkEBCDIC(StringRef Input, const char *Expected, size_t ExpectedLen,
                 bool Ok = true) {
  SmallVector<char, 4096> Out;
  std::error_code EC = ConverterEBCDIC::convertToEBCDIC(Input, Out);
  expect(bool(EC) != Ok, Expected);
  expect(Out.size() == ExpectedLen, Expected);
  if (Out.size() == ExpectedLen)
    expect(std::memcmp(Out.data(), Expected, ExpectedLen) == 0, Expected);
}

void checkUTF8(StringRef Input, const char *Expected, size_t ExpectedLen) {
  SmallVector<char, 4096> Out;
  ConverterEBCDIC::convertToUTF8(Input, Out);
  expect(Out.size() == ExpectedLen, Expected);
  if (Out.size() == ExpectedLen)
    expect(std::memcmp(Out.data(), Expected, ExpectedLen) == 0, Expected);
}

void checkLongASCIIStaysInline() {
  std::string Input(256, static_cast<char>(0xC1));
  SmallVector<char, 256> Out;
  ConverterEBCDIC::convertToUTF8(Input, Out);
  expect(Out.size() == 256, "256-byte EBCDIC ASCII output");
  expect(Out.capacity() == 256, "256-byte EBCDIC ASCII stays inline");
}

} // namespace

int main() {
  checkEBCDIC("ABC", "\xC1\xC2\xC3", 3);
  checkUTF8("\xC1\xC2\xC3", "ABC", 3);
  checkEBCDIC("\xC3\xA9", "\x51", 1);
  checkUTF8("\x51", "\xC3\xA9", 2);
  checkEBCDIC("", "", 0);
  checkUTF8("", "", 0);
  checkEBCDIC("\xC2", "", 0, false);
  checkEBCDIC("\xC2\x41", "", 0, false);
  checkEBCDIC("\x80", "", 0, false);
  checkLongASCIIStaysInline();
  if (Failures != 0)
    return 1;
  std::printf("OK: EBCDIC golden vectors\n");
  return 0;
}

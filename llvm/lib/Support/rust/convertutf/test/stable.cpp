//===-- stable.cpp - Golden tests for the ConvertUTF implementation ------===//
//
// Exercises the public ConvertUTF API through LLVMSupport. The same test is
// run with LLVM_ENABLE_RUST_CONVERTUTF both OFF and ON; the output must be
// identical.
//
//===----------------------------------------------------------------------===//

#include "llvm/Support/ConvertUTF.h"

#include <cstdio>
#include <cstring>

using namespace llvm;

namespace {

int Failures = 0;

#define CHECK(Cond)                                                            \
  do {                                                                         \
    if (!(Cond)) {                                                             \
      std::printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #Cond);             \
      ++Failures;                                                              \
    }                                                                          \
  } while (0)

template <typename T, size_t N>
bool equal(const T *Data, size_t Count, const T (&Expected)[N]) {
  return Count == N && std::memcmp(Data, Expected, sizeof(Expected)) == 0;
}

void testUTF8ToUTF16() {
  const UTF8 S[] = {'A', 0xC3, 0xA9, 0xE2, 0x82,
                    0xAC, 0xF0, 0x9F, 0x98, 0x80, 'Z'};
  const UTF16 Expected[] = {0x41, 0xE9, 0x20AC, 0xD83D, 0xDE00, 0x5A};
  const size_t N = sizeof(S) / sizeof(S[0]);

  UTF16 Out[16] = {};
  const UTF8 *P = S;
  UTF16 *T = Out;
  CHECK(ConvertUTF8toUTF16(&P, S + N, &T, Out + 6, strictConversion) ==
        conversionOK);
  CHECK(P == S + N);
  CHECK(T == Out + 6);
  CHECK(equal(Out, size_t(T - Out), Expected));

  P = S;
  T = Out;
  CHECK(ConvertUTF8toUTF16(&P, S + N, &T, Out + 5, strictConversion) ==
        targetExhausted);
  CHECK(T == Out + 5);
  CHECK(std::memcmp(Out, Expected, 5 * sizeof(UTF16)) == 0);
}

void testUTF8ToUTF32() {
  const UTF8 S[] = {'A', 0xC3, 0xA9, 0xE2, 0x82,
                    0xAC, 0xF0, 0x9F, 0x98, 0x80, 'Z'};
  const UTF32 Expected[] = {0x41, 0xE9, 0x20AC, 0x1F600, 0x5A};
  const size_t N = sizeof(S) / sizeof(S[0]);

  UTF32 Out[16] = {};
  const UTF8 *P = S;
  UTF32 *T = Out;
  CHECK(ConvertUTF8toUTF32(&P, S + N, &T, Out + 5, strictConversion) ==
        conversionOK);
  CHECK(P == S + N);
  CHECK(T == Out + 5);
  CHECK(equal(Out, size_t(T - Out), Expected));

  P = S;
  T = Out;
  CHECK(ConvertUTF8toUTF32Partial(&P, S + N, &T, Out + 5, strictConversion) ==
        conversionOK);
  CHECK(P == S + N);
  CHECK(equal(Out, size_t(T - Out), Expected));
}

void testUTF16AndUTF32ToUTF8() {
  const UTF16 W[] = {0x41, 0xE9, 0x20AC, 0xD83D, 0xDE00, 0x5A};
  const UTF32 U[] = {0x41, 0xE9, 0x20AC, 0x1F600, 0x5A};
  const UTF8 Expected[] = {'A', 0xC3, 0xA9, 0xE2, 0x82,
                           0xAC, 0xF0, 0x9F, 0x98, 0x80, 'Z'};

  UTF8 Out[32] = {};
  const UTF16 *P = W;
  UTF8 *T = Out;
  CHECK(ConvertUTF16toUTF8(&P, W + 6, &T, Out + sizeof(Expected),
                           strictConversion) == conversionOK);
  CHECK(P == W + 6);
  CHECK(equal(Out, size_t(T - Out), Expected));

  std::memset(Out, 0, sizeof(Out));
  const UTF32 *Q = U;
  T = Out;
  CHECK(ConvertUTF32toUTF8(&Q, U + 5, &T, Out + sizeof(Expected),
                           strictConversion) == conversionOK);
  CHECK(Q == U + 5);
  CHECK(equal(Out, size_t(T - Out), Expected));

  const UTF32 FromW[] = {0x41, 0xE9, 0x20AC, 0x1F600, 0x5A};
  UTF32 U32[8] = {};
  P = W;
  UTF32 *R = U32;
  CHECK(ConvertUTF16toUTF32(&P, W + 6, &R, U32 + 5, strictConversion) ==
        conversionOK);
  CHECK(equal(U32, size_t(R - U32), FromW));

  const UTF16 FromU[] = {0x41, 0xE9, 0x20AC, 0xD83D, 0xDE00, 0x5A};
  UTF16 U16[8] = {};
  Q = U;
  UTF16 *S = U16;
  CHECK(ConvertUTF32toUTF16(&Q, U + 5, &S, U16 + 6, strictConversion) ==
        conversionOK);
  CHECK(equal(U16, size_t(S - U16), FromU));
}

void testErrors() {
  {
    const UTF8 S[] = {0xED, 0xA0, 0x80}; // U+D800 as UTF-8
    UTF16 Out[4] = {};
    const UTF8 *P = S;
    UTF16 *T = Out;
    CHECK(ConvertUTF8toUTF16(&P, S + 3, &T, Out + 4, strictConversion) ==
          sourceIllegal);
    CHECK(T == Out);
    P = S;
    T = Out;
    CHECK(ConvertUTF8toUTF16(&P, S + 3, &T, Out + 4, lenientConversion) ==
          sourceIllegal);
    CHECK(T == Out);
  }
  {
    const UTF8 S[] = {0xC0, 0x80}; // overlong NUL
    UTF16 Out[4] = {};
    const UTF8 *P = S;
    UTF16 *T = Out;
    CHECK(ConvertUTF8toUTF16(&P, S + 2, &T, Out + 4, lenientConversion) ==
          sourceIllegal);
    CHECK(T == Out);
    P = S;
    T = Out;
    CHECK(ConvertUTF8toUTF16(&P, S + 2, &T, Out + 4, strictConversion) ==
          sourceIllegal);
    CHECK(T == Out);
  }
  {
    const UTF8 S[] = {0xC0, 0x80}; // overlong NUL
    UTF32 Out[4] = {};
    const UTF8 *P = S;
    UTF32 *T = Out;
    CHECK(ConvertUTF8toUTF32(&P, S + 2, &T, Out + 4, strictConversion) ==
          sourceIllegal);
    CHECK(T == Out);
    P = S;
    T = Out;
    CHECK(ConvertUTF8toUTF32(&P, S + 2, &T, Out + 4, lenientConversion) ==
          sourceIllegal);
    CHECK(T == Out + 2 && Out[0] == 0xFFFD && Out[1] == 0xFFFD);
  }
  {
    const UTF8 S[] = {0xE2, 0x82}; // truncated
    UTF16 Out[4] = {};
    const UTF8 *P = S;
    UTF16 *T = Out;
    CHECK(ConvertUTF8toUTF16(&P, S + 2, &T, Out + 4, strictConversion) ==
          sourceExhausted);
    CHECK(T == Out);
    UTF32 Out32[4] = {};
    P = S;
    UTF32 *T32 = Out32;
    CHECK(ConvertUTF8toUTF32(&P, S + 2, &T32, Out32 + 4, strictConversion) ==
          sourceExhausted);
    CHECK(T32 == Out32);
    P = S;
    T32 = Out32;
    CHECK(ConvertUTF8toUTF32(&P, S + 2, &T32, Out32 + 4, lenientConversion) ==
          sourceIllegal);
    CHECK(T32 == Out32 + 1 && Out32[0] == 0xFFFD);
  }
  {
    const UTF32 U[] = {0x110000};
    UTF8 Out[8] = {};
    const UTF32 *P = U;
    UTF8 *T = Out;
    CHECK(ConvertUTF32toUTF8(&P, U + 1, &T, Out + 8, strictConversion) ==
          sourceIllegal);
    CHECK(T == Out + 3);
    CHECK(Out[0] == 0xEF && Out[1] == 0xBF && Out[2] == 0xBD);
    UTF16 Out16[4] = {};
    P = U;
    UTF16 *T16 = Out16;
    CHECK(ConvertUTF32toUTF16(&P, U + 1, &T16, Out16 + 4, strictConversion) ==
          sourceIllegal);
    CHECK(T16 == Out16);
    P = U;
    T16 = Out16;
    CHECK(ConvertUTF32toUTF16(&P, U + 1, &T16, Out16 + 4, lenientConversion) ==
          conversionOK);
    CHECK(T16 == Out16 + 1 && Out16[0] == 0xFFFD);
  }
  {
    const UTF32 U[] = {0xD800};
    UTF8 Out[8] = {};
    const UTF32 *P = U;
    UTF8 *T = Out;
    CHECK(ConvertUTF32toUTF8(&P, U + 1, &T, Out + 8, strictConversion) ==
          sourceIllegal);
    CHECK(T == Out);
    P = U;
    T = Out;
    CHECK(ConvertUTF32toUTF8(&P, U + 1, &T, Out + 8, lenientConversion) ==
          conversionOK);
    CHECK(T == Out + 3);
    CHECK(Out[0] == 0xED && Out[1] == 0xA0 && Out[2] == 0x80);
  }
  {
    const UTF16 W[] = {0xD800, 0x0041};
    UTF8 Out[8] = {};
    const UTF16 *P = W;
    UTF8 *T = Out;
    CHECK(ConvertUTF16toUTF8(&P, W + 2, &T, Out + 8, strictConversion) ==
          sourceIllegal);
    CHECK(T == Out);
    P = W;
    T = Out;
    CHECK(ConvertUTF16toUTF8(&P, W + 2, &T, Out + 8, lenientConversion) ==
          conversionOK);
    CHECK(T == Out + 4);
    CHECK(Out[0] == 0xED && Out[1] == 0xA0 && Out[2] == 0x80 && Out[3] == 0x41);
    UTF32 Out32[4] = {};
    P = W;
    UTF32 *T32 = Out32;
    CHECK(ConvertUTF16toUTF32(&P, W + 2, &T32, Out32 + 4, lenientConversion) ==
          conversionOK);
    CHECK(T32 == Out32 + 2 && Out32[0] == 0xD800 && Out32[1] == 0x41);
  }
}

void testHelpers() {
  const UTF8 Valid[] = {'A', 0xC3, 0xA9, 0xE2, 0x82, 0xAC,
                        0xF0, 0x9F, 0x98, 0x80};
  const UTF8 *P = Valid;
  CHECK(isLegalUTF8String(&P, Valid + sizeof(Valid)) == true);
  CHECK(P == Valid + sizeof(Valid));

  const UTF8 Invalid[] = {0xC0, 0x80};
  P = Invalid;
  CHECK(isLegalUTF8String(&P, Invalid + 2) == false);
  CHECK(P == Invalid);

  CHECK(isLegalUTF8Sequence(Valid + 6, Valid + 10) == true);
  CHECK(getUTF8SequenceSize(Valid + 6, Valid + 10) == 4);
  CHECK(getUTF8SequenceSize(Valid + 6, Valid + 9) == 0);

  const UTF8 Overlong[] = {0xE0, 0x80, 0x80};
  CHECK(isLegalUTF8Sequence(Overlong, Overlong + 3) == false);
  CHECK(findMaximalSubpartOfIllFormedUTF8Sequence(Overlong, Overlong + 3) == 1);

  const UTF8 Truncated[] = {0xF0, 0x9F, 0x98};
  CHECK(isLegalUTF8Sequence(Truncated, Truncated + 3) == false);
  CHECK(findMaximalSubpartOfIllFormedUTF8Sequence(Truncated, Truncated + 3) ==
        3);

  CHECK(getNumBytesForUTF8(0x00) == 1);
  CHECK(getNumBytesForUTF8(0x7F) == 1);
  CHECK(getNumBytesForUTF8(0xC2) == 2);
  CHECK(getNumBytesForUTF8(0xE0) == 3);
  CHECK(getNumBytesForUTF8(0xF0) == 4);
  CHECK(getNumBytesForUTF8(0xFF) == 6);
}

} // namespace

int main() {
  testUTF8ToUTF16();
  testUTF8ToUTF32();
  testUTF16AndUTF32ToUTF8();
  testErrors();
  testHelpers();
  if (Failures) {
    std::printf("%d failures\n", Failures);
    return 1;
  }
  std::printf("stable OK\n");
  return 0;
}

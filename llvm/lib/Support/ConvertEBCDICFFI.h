//===-- ConvertEBCDICFFI.h - FFI boundary for EBCDIC conversion --*- C -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//

#ifndef LLVM_LIB_SUPPORT_CONVERTEBCDICFFI_H
#define LLVM_LIB_SUPPORT_CONVERTEBCDICFFI_H

#include <cstddef>
#include <cstdint>

extern "C" int32_t llvm_convertebcdic_to_ebcdic(const uint8_t *Source,
                                                size_t SourceLen,
                                                uint8_t *Result,
                                                size_t ResultCap,
                                                size_t *ResultLen);
extern "C" int32_t llvm_convertebcdic_to_utf8(const uint8_t *Source,
                                              size_t SourceLen, uint8_t *Result,
                                              size_t ResultCap,
                                              size_t *ResultLen);

#endif // LLVM_LIB_SUPPORT_CONVERTEBCDICFFI_H

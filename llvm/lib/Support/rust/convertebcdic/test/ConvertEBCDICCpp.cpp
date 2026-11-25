//===-- ConvertEBCDICCpp.cpp - Renamed C++ EBCDIC reference ---------------===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Build the original C++ implementation under a second namespace so the
// differential harness can compare it with the Rust-backed API.
//
//===----------------------------------------------------------------------===//

#undef LLVM_RUST_CONVERTEBCDIC
#define ConverterEBCDIC ConverterEBCDICCpp
#include "../../../ConvertEBCDIC.cpp"

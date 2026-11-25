//===-- lib.rs - Rust port of LLVM's xxHash --------------------*- Rust -*-===//
//
// Part of the LLVM Project, under the Apache License v2.0 with LLVM Exceptions.
// See https://llvm.org/LICENSE.txt for license information.
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
//
//===----------------------------------------------------------------------===//
//
// Bit-compatible reimplementation of the XXH3 64-bit and 128-bit hashes in
// llvm/lib/Support/xxhash.cpp. The scalar paths mirror the C++ code. On
// x86-64 the long-input paths use AVX2 when the CPU and OS support it, which
// is the part that makes the Rust port faster than the scalar C++ loop.
//
//===----------------------------------------------------------------------===//

// This crate is a translation of xxHash, which is distributed under the
// BSD 2-Clause License:
//
// xxHash - Extremely Fast Hash algorithm
// Copyright (C) 2012-2023, Yann Collet
//
// BSD 2-Clause License (http://www.opensource.org/licenses/bsd-license.php)
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are
// met:
//
// * Redistributions of source code must retain the above copyright
// notice, this list of conditions and the following disclaimer.
// * Redistributions in binary form must reproduce the above
// copyright notice, this list of conditions and the following disclaimer
// in the documentation and/or other materials provided with the
// distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
// OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
// LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
// DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
// THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
//
// You can contact the author at :
// - xxHash homepage: http://www.xxhash.com
// - xxHash source repository : https://github.com/Cyan4973/xxHash
//
// The 64-bit paths follow xxHash commit d5891596637d21366b9b1dcf2c0007a3edb26a9e
// (July 2023); the 128-bit paths follow commit
// b0adcc54188c3130b1793e7b19c62eb1e669f7df (June 2024).

#![cfg_attr(not(test), no_std)]
#![allow(clippy::missing_safety_doc)]
// The raw-pointer helpers are unsafe by contract; keep their bodies readable.
#![allow(unsafe_op_in_unsafe_fn)]

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    unsafe extern "C" {
        fn abort() -> !;
    }
    unsafe { abort() }
}

const PRIME32_1: u32 = 0x9E37_79B1;
const PRIME32_2: u32 = 0x85EB_CA77;
const PRIME32_3: u32 = 0xC2B2_AE3D;

const PRIME64_1: u64 = 11_400_714_785_074_694_791;
const PRIME64_2: u64 = 14_029_467_366_897_019_727;
const PRIME64_3: u64 = 1_609_587_929_392_839_161;
const PRIME64_4: u64 = 9_650_029_242_287_828_579;
const PRIME64_5: u64 = 2_870_177_450_012_600_261;

const PRIME_MX1: u64 = 0x1656_6791_9E37_79F9;
const PRIME_MX2: u64 = 0x9FB2_1C65_1E98_DF25;

const XXH_SECRET_DEFAULT_SIZE: usize = 192;
const XXH3_SECRETSIZE_MIN: usize = 136;
const XXH_STRIPE_LEN: usize = 64;
const XXH_SECRET_CONSUME_RATE: usize = 8;
const XXH_ACC_NB: usize = 8;
const XXH3_MIDSIZE_MAX: usize = 240;
const XXH3_MIDSIZE_STARTOFFSET: usize = 3;
const XXH3_MIDSIZE_LASTOFFSET: usize = 17;

// Pseudorandom data taken directly from FARSH.
#[rustfmt::skip]
const K_SECRET: [u8; XXH_SECRET_DEFAULT_SIZE] = [
    0xb8, 0xfe, 0x6c, 0x39, 0x23, 0xa4, 0x4b, 0xbe, 0x7c, 0x01, 0x81, 0x2c, 0xf7, 0x21, 0xad, 0x1c,
    0xde, 0xd4, 0x6d, 0xe9, 0x83, 0x90, 0x97, 0xdb, 0x72, 0x40, 0xa4, 0xa4, 0xb7, 0xb3, 0x67, 0x1f,
    0xcb, 0x79, 0xe6, 0x4e, 0xcc, 0xc0, 0xe5, 0x78, 0x82, 0x5a, 0xd0, 0x7d, 0xcc, 0xff, 0x72, 0x21,
    0xb8, 0x08, 0x46, 0x74, 0xf7, 0x43, 0x24, 0x8e, 0xe0, 0x35, 0x90, 0xe6, 0x81, 0x3a, 0x26, 0x4c,
    0x3c, 0x28, 0x52, 0xbb, 0x91, 0xc3, 0x00, 0xcb, 0x88, 0xd0, 0x65, 0x8b, 0x1b, 0x53, 0x2e, 0xa3,
    0x71, 0x64, 0x48, 0x97, 0xa2, 0x0d, 0xf9, 0x4e, 0x38, 0x19, 0xef, 0x46, 0xa9, 0xde, 0xac, 0xd8,
    0xa8, 0xfa, 0x76, 0x3f, 0xe3, 0x9c, 0x34, 0x3f, 0xf9, 0xdc, 0xbb, 0xc7, 0xc7, 0x0b, 0x4f, 0x1d,
    0x8a, 0x51, 0xe0, 0x4b, 0xcd, 0xb4, 0x59, 0x31, 0xc8, 0x9f, 0x7e, 0xc9, 0xd9, 0x78, 0x73, 0x64,
    0xea, 0xc5, 0xac, 0x83, 0x34, 0xd3, 0xeb, 0xc3, 0xc5, 0x81, 0xa0, 0xff, 0xfa, 0x13, 0x63, 0xeb,
    0x17, 0x0d, 0xdd, 0x51, 0xb7, 0xf0, 0xda, 0x49, 0xd3, 0x16, 0x55, 0x26, 0x29, 0xd4, 0x68, 0x9e,
    0x2b, 0x16, 0xbe, 0x58, 0x7d, 0x47, 0xa1, 0xfc, 0x8f, 0xf8, 0xb8, 0xd1, 0x7a, 0xd0, 0x31, 0xce,
    0x45, 0xcb, 0x3a, 0x8f, 0x95, 0x16, 0x04, 0x28, 0xaf, 0xd7, 0xfb, 0xca, 0xbb, 0x4b, 0x40, 0x7e,
];

#[inline(always)]
fn read32le(p: *const u8) -> u32 {
    unsafe { core::ptr::read_unaligned(p as *const u32) }.to_le()
}

#[inline(always)]
fn read64le(p: *const u8) -> u64 {
    unsafe { core::ptr::read_unaligned(p as *const u64) }.to_le()
}

#[inline(always)]
fn xxh64_avalanche(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(PRIME64_2);
    h ^= h >> 29;
    h = h.wrapping_mul(PRIME64_3);
    h ^= h >> 32;
    h
}

#[inline(always)]
fn xxh3_avalanche(mut h: u64) -> u64 {
    h ^= h >> 37;
    h = h.wrapping_mul(PRIME_MX1);
    h ^= h >> 32;
    h
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hash128 {
    low: u64,
    high: u64,
}

#[inline(always)]
fn mul128_fold64(lhs: u64, rhs: u64) -> u64 {
    let product = (lhs as u128) * (rhs as u128);
    (product as u64) ^ ((product >> 64) as u64)
}

#[inline(always)]
fn mult64to128(lhs: u64, rhs: u64) -> Hash128 {
    let product = (lhs as u128) * (rhs as u128);
    Hash128 {
        low: product as u64,
        high: (product >> 64) as u64,
    }
}

#[inline(always)]
fn xorshift64(v: u64, shift: u32) -> u64 {
    v ^ (v >> shift)
}

#[inline(always)]
fn xxh3_len_1to3_64b(input: *const u8, len: usize, secret: *const u8, seed: u64) -> u64 {
    let c1 = unsafe { *input };
    let c2 = unsafe { *input.add(len >> 1) };
    let c3 = unsafe { *input.add(len - 1) };
    let combined = ((c1 as u32) << 16) | ((c2 as u32) << 24) | (c3 as u32) | ((len as u32) << 8);
    let bitflip =
        ((read32le(secret) ^ read32le(unsafe { secret.add(4) })) as u64).wrapping_add(seed);
    xxh64_avalanche((combined as u64) ^ bitflip)
}

#[inline(always)]
fn xxh3_len_4to8_64b(input: *const u8, len: usize, secret: *const u8, seed: u64) -> u64 {
    let seed = seed ^ (((seed as u32).swap_bytes() as u64) << 32);
    let input1 = read32le(input);
    let input2 = read32le(unsafe { input.add(len - 4) });
    let mut acc = (read64le(unsafe { secret.add(8) }) ^ read64le(unsafe { secret.add(16) }))
        .wrapping_sub(seed);
    let input64 = (input2 as u64) | ((input1 as u64) << 32);
    acc ^= input64;
    acc ^= acc.rotate_left(49) ^ acc.rotate_left(24);
    acc = acc.wrapping_mul(PRIME_MX2);
    acc ^= (acc >> 35).wrapping_add(len as u64);
    acc = acc.wrapping_mul(PRIME_MX2);
    acc ^ (acc >> 28)
}

#[inline(always)]
fn xxh3_len_9to16_64b(input: *const u8, len: usize, secret: *const u8, seed: u64) -> u64 {
    let mut input_lo = (read64le(unsafe { secret.add(24) }) ^ read64le(unsafe { secret.add(32) }))
        .wrapping_add(seed);
    let mut input_hi = (read64le(unsafe { secret.add(40) }) ^ read64le(unsafe { secret.add(48) }))
        .wrapping_sub(seed);
    input_lo ^= read64le(input);
    input_hi ^= read64le(unsafe { input.add(len - 8) });
    let acc = (len as u64)
        .wrapping_add(input_lo.swap_bytes())
        .wrapping_add(input_hi)
        .wrapping_add(mul128_fold64(input_lo, input_hi));
    xxh3_avalanche(acc)
}

#[inline(always)]
fn xxh3_len_0to16_64b(input: *const u8, len: usize, secret: *const u8, seed: u64) -> u64 {
    if len > 8 {
        return xxh3_len_9to16_64b(input, len, secret, seed);
    }
    if len >= 4 {
        return xxh3_len_4to8_64b(input, len, secret, seed);
    }
    if len != 0 {
        return xxh3_len_1to3_64b(input, len, secret, seed);
    }
    xxh64_avalanche(
        seed ^ read64le(unsafe { secret.add(56) }) ^ read64le(unsafe { secret.add(64) }),
    )
}

#[inline(always)]
fn xxh3_mix16b(input: *const u8, secret: *const u8, seed: u64) -> u64 {
    let mut lhs = seed.wrapping_add(read64le(secret));
    let mut rhs = (0u64.wrapping_sub(seed)).wrapping_add(read64le(unsafe { secret.add(8) }));
    lhs ^= read64le(input);
    rhs ^= read64le(unsafe { input.add(8) });
    mul128_fold64(lhs, rhs)
}

#[inline(always)]
fn xxh3_len_17to128_64b(input: *const u8, len: usize, secret: *const u8, seed: u64) -> u64 {
    let mut acc = (len as u64).wrapping_mul(PRIME64_1);
    acc = acc.wrapping_add(xxh3_mix16b(input, secret, seed));
    let mut acc_end = xxh3_mix16b(
        unsafe { input.add(len - 16) },
        unsafe { secret.add(16) },
        seed,
    );
    if len > 32 {
        acc = acc.wrapping_add(xxh3_mix16b(
            unsafe { input.add(16) },
            unsafe { secret.add(32) },
            seed,
        ));
        acc_end = acc_end.wrapping_add(xxh3_mix16b(
            unsafe { input.add(len - 32) },
            unsafe { secret.add(48) },
            seed,
        ));
        if len > 64 {
            acc = acc.wrapping_add(xxh3_mix16b(
                unsafe { input.add(32) },
                unsafe { secret.add(64) },
                seed,
            ));
            acc_end = acc_end.wrapping_add(xxh3_mix16b(
                unsafe { input.add(len - 48) },
                unsafe { secret.add(80) },
                seed,
            ));
            if len > 96 {
                acc = acc.wrapping_add(xxh3_mix16b(
                    unsafe { input.add(48) },
                    unsafe { secret.add(96) },
                    seed,
                ));
                acc_end = acc_end.wrapping_add(xxh3_mix16b(
                    unsafe { input.add(len - 64) },
                    unsafe { secret.add(112) },
                    seed,
                ));
            }
        }
    }
    xxh3_avalanche(acc.wrapping_add(acc_end))
}

#[inline(never)]
fn xxh3_len_129to240_64b(input: *const u8, len: usize, secret: *const u8, seed: u64) -> u64 {
    let mut acc = (len as u64).wrapping_mul(PRIME64_1);
    let nb_rounds = len / 16;
    for i in 0..8 {
        acc = acc.wrapping_add(xxh3_mix16b(
            unsafe { input.add(16 * i) },
            unsafe { secret.add(16 * i) },
            seed,
        ));
    }
    acc = xxh3_avalanche(acc);
    for i in 8..nb_rounds {
        acc = acc.wrapping_add(xxh3_mix16b(
            unsafe { input.add(16 * i) },
            unsafe { secret.add(16 * (i - 8) + XXH3_MIDSIZE_STARTOFFSET) },
            seed,
        ));
    }
    acc = acc.wrapping_add(xxh3_mix16b(
        unsafe { input.add(len - 16) },
        unsafe { secret.add(XXH3_SECRETSIZE_MIN - XXH3_MIDSIZE_LASTOFFSET) },
        seed,
    ));
    xxh3_avalanche(acc)
}

#[inline(always)]
unsafe fn xxh3_accumulate_512_scalar(acc: *mut u64, input: *const u8, secret: *const u8) {
    for i in 0..XXH_ACC_NB {
        let data_val = read64le(input.add(8 * i));
        let data_key = data_val ^ read64le(secret.add(8 * i));
        let swapped = i ^ 1;
        *acc.add(swapped) = (*acc.add(swapped)).wrapping_add(data_val);
        *acc.add(i) =
            (*acc.add(i)).wrapping_add((data_key as u32 as u64).wrapping_mul(data_key >> 32));
    }
}

#[inline(always)]
unsafe fn xxh3_scramble_acc_scalar(acc: *mut u64, secret: *const u8) {
    for i in 0..XXH_ACC_NB {
        let mut value = *acc.add(i);
        value ^= value >> 47;
        value ^= read64le(secret.add(8 * i));
        *acc.add(i) = value.wrapping_mul(PRIME32_1 as u64);
    }
}

#[inline(always)]
unsafe fn xxh3_accumulate_scalar(
    acc: *mut u64,
    input: *const u8,
    secret: *const u8,
    nb_stripes: usize,
) {
    for n in 0..nb_stripes {
        xxh3_accumulate_512_scalar(
            acc,
            input.add(n * XXH_STRIPE_LEN),
            secret.add(n * XXH_SECRET_CONSUME_RATE),
        );
    }
}

#[inline(always)]
fn xxh3_mix2accs(acc: *const u64, secret: *const u8) -> u64 {
    mul128_fold64(
        unsafe { *acc } ^ read64le(secret),
        unsafe { *acc.add(1) } ^ read64le(unsafe { secret.add(8) }),
    )
}

#[inline(always)]
fn xxh3_merge_accs(acc: *const u64, key: *const u8, start: u64) -> u64 {
    let mut result = start;
    for i in 0..4 {
        result = result.wrapping_add(xxh3_mix2accs(unsafe { acc.add(2 * i) }, unsafe {
            key.add(16 * i)
        }));
    }
    xxh3_avalanche(result)
}

#[inline(never)]
fn xxh3_hash_long_64b(input: *const u8, len: usize, secret: *const u8, secret_size: usize) -> u64 {
    let nb_stripes_per_block = (secret_size - XXH_STRIPE_LEN) / XXH_SECRET_CONSUME_RATE;
    let block_len = XXH_STRIPE_LEN * nb_stripes_per_block;
    let nb_blocks = (len - 1) / block_len;
    let mut acc: [u64; XXH_ACC_NB] = [
        PRIME32_3 as u64,
        PRIME64_1,
        PRIME64_2,
        PRIME64_3,
        PRIME64_4,
        PRIME32_2 as u64,
        PRIME64_5,
        PRIME32_1 as u64,
    ];

    for n in 0..nb_blocks {
        unsafe {
            xxh3_accumulate_scalar(
                acc.as_mut_ptr(),
                input.add(n * block_len),
                secret,
                nb_stripes_per_block,
            );
            xxh3_scramble_acc_scalar(acc.as_mut_ptr(), secret.add(secret_size - XXH_STRIPE_LEN));
        }
    }

    let nb_stripes = (len - 1 - block_len * nb_blocks) / XXH_STRIPE_LEN;
    unsafe {
        xxh3_accumulate_scalar(
            acc.as_mut_ptr(),
            input.add(nb_blocks * block_len),
            secret,
            nb_stripes,
        );
        xxh3_accumulate_512_scalar(
            acc.as_mut_ptr(),
            input.add(len - XXH_STRIPE_LEN),
            secret.add(secret_size - XXH_STRIPE_LEN - 7),
        );
    }

    xxh3_merge_accs(
        acc.as_ptr(),
        unsafe { secret.add(11) },
        (len as u64).wrapping_mul(PRIME64_1),
    )
}

#[inline(always)]
fn xxh3_64bits_scalar(input: *const u8, len: usize) -> u64 {
    let secret = K_SECRET.as_ptr();
    if len <= 16 {
        return xxh3_len_0to16_64b(input, len, secret, 0);
    }
    if len <= 128 {
        return xxh3_len_17to128_64b(input, len, secret, 0);
    }
    if len <= XXH3_MIDSIZE_MAX {
        return xxh3_len_129to240_64b(input, len, secret, 0);
    }
    xxh3_hash_long_64b(input, len, secret, XXH_SECRET_DEFAULT_SIZE)
}

#[inline(always)]
pub(crate) fn xxh3_64bits(input: *const u8, len: usize) -> u64 {
    if len <= XXH3_MIDSIZE_MAX {
        return xxh3_64bits_scalar(input, len);
    }
    let secret = K_SECRET.as_ptr();
    #[cfg(target_arch = "x86_64")]
    if has_avx2() {
        return unsafe { xxh3_hash_long_64b_avx2(input, len, secret, XXH_SECRET_DEFAULT_SIZE) };
    }
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    return unsafe { xxh3_hash_long_64b_neon(input, len, secret, XXH_SECRET_DEFAULT_SIZE) };
    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    xxh3_hash_long_64b(input, len, secret, XXH_SECRET_DEFAULT_SIZE)
}

#[inline(always)]
fn xxh3_len_1to3_128b(input: *const u8, len: usize, secret: *const u8, seed: u64) -> Hash128 {
    let c1 = unsafe { *input };
    let c2 = unsafe { *input.add(len >> 1) };
    let c3 = unsafe { *input.add(len - 1) };
    let combinedl = ((c1 as u32) << 16) | ((c2 as u32) << 24) | (c3 as u32) | ((len as u32) << 8);
    let combinedh = combinedl.swap_bytes().rotate_left(13);
    let bitflipl =
        ((read32le(secret) ^ read32le(unsafe { secret.add(4) })) as u64).wrapping_add(seed);
    let bitfliph = ((read32le(unsafe { secret.add(8) }) ^ read32le(unsafe { secret.add(12) }))
        as u64)
        .wrapping_sub(seed);
    Hash128 {
        low: xxh64_avalanche((combinedl as u64) ^ bitflipl),
        high: xxh64_avalanche((combinedh as u64) ^ bitfliph),
    }
}

#[inline(always)]
fn xxh3_len_4to8_128b(input: *const u8, len: usize, secret: *const u8, seed: u64) -> Hash128 {
    let seed = seed ^ (((seed as u32).swap_bytes() as u64) << 32);
    let input_lo = read32le(input);
    let input_hi = read32le(unsafe { input.add(len - 4) });
    let input_64 = (input_lo as u64).wrapping_add((input_hi as u64) << 32);
    let bitflip = (read64le(unsafe { secret.add(16) }) ^ read64le(unsafe { secret.add(24) }))
        .wrapping_add(seed);
    let keyed = input_64 ^ bitflip;
    let mut m128 = mult64to128(keyed, PRIME64_1.wrapping_add((len as u64) << 2));
    m128.high = m128.high.wrapping_add(m128.low << 1);
    m128.low ^= m128.high >> 3;
    m128.low = xorshift64(m128.low, 35);
    m128.low = m128.low.wrapping_mul(PRIME_MX2);
    m128.low = xorshift64(m128.low, 28);
    m128.high = xxh3_avalanche(m128.high);
    m128
}

#[inline(always)]
fn xxh3_len_9to16_128b(input: *const u8, len: usize, secret: *const u8, seed: u64) -> Hash128 {
    let bitflipl = (read64le(unsafe { secret.add(32) }) ^ read64le(unsafe { secret.add(40) }))
        .wrapping_sub(seed);
    let bitfliph = (read64le(unsafe { secret.add(48) }) ^ read64le(unsafe { secret.add(56) }))
        .wrapping_add(seed);
    let input_lo = read64le(input);
    let mut input_hi = read64le(unsafe { input.add(len - 8) });
    let mut m128 = mult64to128(input_lo ^ input_hi ^ bitflipl, PRIME64_1);
    m128.low = m128.low.wrapping_add(((len as u64) - 1) << 54);
    input_hi ^= bitfliph;
    m128.high = m128
        .high
        .wrapping_add(input_hi)
        .wrapping_add((input_hi as u32 as u64).wrapping_mul((PRIME32_2 - 1) as u64));
    m128.low ^= m128.high.swap_bytes();
    let mut h128 = mult64to128(m128.low, PRIME64_2);
    h128.high = h128.high.wrapping_add(m128.high.wrapping_mul(PRIME64_2));
    h128.low = xxh3_avalanche(h128.low);
    h128.high = xxh3_avalanche(h128.high);
    h128
}

#[inline(always)]
fn xxh3_len_0to16_128b(input: *const u8, len: usize, secret: *const u8, seed: u64) -> Hash128 {
    if len > 8 {
        return xxh3_len_9to16_128b(input, len, secret, seed);
    }
    if len >= 4 {
        return xxh3_len_4to8_128b(input, len, secret, seed);
    }
    if len != 0 {
        return xxh3_len_1to3_128b(input, len, secret, seed);
    }
    let bitflipl = read64le(unsafe { secret.add(64) }) ^ read64le(unsafe { secret.add(72) });
    let bitfliph = read64le(unsafe { secret.add(80) }) ^ read64le(unsafe { secret.add(88) });
    Hash128 {
        low: xxh64_avalanche(seed ^ bitflipl),
        high: xxh64_avalanche(seed ^ bitfliph),
    }
}

#[inline(always)]
fn xxh128_mix32b(
    mut acc: Hash128,
    input_1: *const u8,
    input_2: *const u8,
    secret: *const u8,
    seed: u64,
) -> Hash128 {
    acc.low = acc.low.wrapping_add(xxh3_mix16b(input_1, secret, seed));
    acc.low ^= read64le(input_2).wrapping_add(read64le(unsafe { input_2.add(8) }));
    acc.high = acc
        .high
        .wrapping_add(xxh3_mix16b(input_2, unsafe { secret.add(16) }, seed));
    acc.high ^= read64le(input_1).wrapping_add(read64le(unsafe { input_1.add(8) }));
    acc
}

#[inline(always)]
fn xxh3_len_17to128_128b(input: *const u8, len: usize, secret: *const u8, seed: u64) -> Hash128 {
    let mut acc = Hash128 {
        low: (len as u64).wrapping_mul(PRIME64_1),
        high: 0,
    };
    if len > 32 {
        if len > 64 {
            if len > 96 {
                acc = xxh128_mix32b(
                    acc,
                    unsafe { input.add(48) },
                    unsafe { input.add(len - 64) },
                    unsafe { secret.add(96) },
                    seed,
                );
            }
            acc = xxh128_mix32b(
                acc,
                unsafe { input.add(32) },
                unsafe { input.add(len - 48) },
                unsafe { secret.add(64) },
                seed,
            );
        }
        acc = xxh128_mix32b(
            acc,
            unsafe { input.add(16) },
            unsafe { input.add(len - 32) },
            unsafe { secret.add(32) },
            seed,
        );
    }
    acc = xxh128_mix32b(acc, input, unsafe { input.add(len - 16) }, secret, seed);
    let mut h128 = Hash128 {
        low: acc.low.wrapping_add(acc.high),
        high: acc
            .low
            .wrapping_mul(PRIME64_1)
            .wrapping_add(acc.high.wrapping_mul(PRIME64_4))
            .wrapping_add((len as u64).wrapping_sub(seed).wrapping_mul(PRIME64_2)),
    };
    h128.low = xxh3_avalanche(h128.low);
    h128.high = 0u64.wrapping_sub(xxh3_avalanche(h128.high));
    h128
}

#[inline(never)]
fn xxh3_len_129to240_128b(input: *const u8, len: usize, secret: *const u8, seed: u64) -> Hash128 {
    let mut acc = Hash128 {
        low: (len as u64).wrapping_mul(PRIME64_1),
        high: 0,
    };
    let mut i = 32;
    while i < 160 {
        acc = xxh128_mix32b(
            acc,
            unsafe { input.add(i - 32) },
            unsafe { input.add(i - 16) },
            unsafe { secret.add(i - 32) },
            seed,
        );
        i += 32;
    }
    acc.low = xxh3_avalanche(acc.low);
    acc.high = xxh3_avalanche(acc.high);
    let mut i = 160;
    while i <= len {
        acc = xxh128_mix32b(
            acc,
            unsafe { input.add(i - 32) },
            unsafe { input.add(i - 16) },
            unsafe { secret.add(XXH3_MIDSIZE_STARTOFFSET + i - 160) },
            seed,
        );
        i += 32;
    }
    acc = xxh128_mix32b(
        acc,
        unsafe { input.add(len - 16) },
        unsafe { input.add(len - 32) },
        unsafe { secret.add(XXH3_SECRETSIZE_MIN - XXH3_MIDSIZE_LASTOFFSET - 16) },
        0u64.wrapping_sub(seed),
    );
    let mut h128 = Hash128 {
        low: acc.low.wrapping_add(acc.high),
        high: acc
            .low
            .wrapping_mul(PRIME64_1)
            .wrapping_add(acc.high.wrapping_mul(PRIME64_4))
            .wrapping_add((len as u64).wrapping_sub(seed).wrapping_mul(PRIME64_2)),
    };
    h128.low = xxh3_avalanche(h128.low);
    h128.high = 0u64.wrapping_sub(xxh3_avalanche(h128.high));
    h128
}

#[inline(never)]
fn xxh3_hash_long_128b(
    input: *const u8,
    len: usize,
    secret: *const u8,
    secret_size: usize,
) -> Hash128 {
    let nb_stripes_per_block = (secret_size - XXH_STRIPE_LEN) / XXH_SECRET_CONSUME_RATE;
    let block_len = XXH_STRIPE_LEN * nb_stripes_per_block;
    let nb_blocks = (len - 1) / block_len;
    let mut acc: [u64; XXH_ACC_NB] = [
        PRIME32_3 as u64,
        PRIME64_1,
        PRIME64_2,
        PRIME64_3,
        PRIME64_4,
        PRIME32_2 as u64,
        PRIME64_5,
        PRIME32_1 as u64,
    ];

    for n in 0..nb_blocks {
        unsafe {
            xxh3_accumulate_scalar(
                acc.as_mut_ptr(),
                input.add(n * block_len),
                secret,
                nb_stripes_per_block,
            );
            xxh3_scramble_acc_scalar(acc.as_mut_ptr(), secret.add(secret_size - XXH_STRIPE_LEN));
        }
    }

    let nb_stripes = (len - 1 - block_len * nb_blocks) / XXH_STRIPE_LEN;
    unsafe {
        xxh3_accumulate_scalar(
            acc.as_mut_ptr(),
            input.add(nb_blocks * block_len),
            secret,
            nb_stripes,
        );
        xxh3_accumulate_512_scalar(
            acc.as_mut_ptr(),
            input.add(len - XXH_STRIPE_LEN),
            secret.add(secret_size - XXH_STRIPE_LEN - 7),
        );
    }

    let low = xxh3_merge_accs(
        acc.as_ptr(),
        unsafe { secret.add(11) },
        (len as u64).wrapping_mul(PRIME64_1),
    );
    let high = xxh3_merge_accs(
        acc.as_ptr(),
        unsafe { secret.add(secret_size - XXH_STRIPE_LEN - 11) },
        !(len as u64).wrapping_mul(PRIME64_2),
    );
    Hash128 { low, high }
}

#[cfg(target_arch = "x86_64")]
use core::sync::atomic::{AtomicU8, Ordering};

// 0 = unknown, 1 = no AVX2, 2 = AVX2. CPUID is serializing and expensive in
// virtualized guests, so pay for it once instead of on every long hash.
#[cfg(target_arch = "x86_64")]
static AVX2_STATE: AtomicU8 = AtomicU8::new(0);

#[cfg(target_arch = "x86_64")]
fn has_avx2() -> bool {
    match AVX2_STATE.load(Ordering::Relaxed) {
        1 => return false,
        2 => return true,
        _ => {}
    }
    let detected = detect_avx2();
    AVX2_STATE.store(if detected { 2 } else { 1 }, Ordering::Relaxed);
    detected
}

#[cfg(target_arch = "x86_64")]
fn detect_avx2() -> bool {
    use core::arch::x86_64::{__cpuid, __cpuid_count, _xgetbv};
    unsafe {
        if __cpuid(0).eax < 7 {
            return false;
        }
        let leaf1 = __cpuid(1);
        if (leaf1.ecx & (1 << 27)) == 0 || (leaf1.ecx & (1 << 28)) == 0 {
            return false;
        }
        if (_xgetbv(0) & 0x6) != 0x6 {
            return false;
        }
        (__cpuid_count(7, 0).ebx & (1 << 5)) != 0
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn xxh3_accumulate_512_avx2(acc: *mut u64, input: *const u8, secret: *const u8) {
    use core::arch::x86_64::*;
    let mut i = 0;
    while i < XXH_ACC_NB {
        let data = _mm256_loadu_si256(input.add(8 * i) as *const __m256i);
        let key = _mm256_loadu_si256(secret.add(8 * i) as *const __m256i);
        let data_key = _mm256_xor_si256(data, key);
        let prod = _mm256_mul_epu32(data_key, _mm256_srli_epi64::<32>(data_key));
        let swapped = _mm256_shuffle_epi32::<0x4E>(data);
        let acc_vec = _mm256_loadu_si256(acc.add(i) as *const __m256i);
        let acc_vec = _mm256_add_epi64(acc_vec, swapped);
        let acc_vec = _mm256_add_epi64(acc_vec, prod);
        _mm256_storeu_si256(acc.add(i) as *mut __m256i, acc_vec);
        i += 4;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn xxh3_scramble_acc_avx2(acc: *mut u64, secret: *const u8) {
    use core::arch::x86_64::*;
    let prime = _mm256_set1_epi64x(PRIME32_1 as i64);
    let mut i = 0;
    while i < XXH_ACC_NB {
        let acc_vec = _mm256_loadu_si256(acc.add(i) as *const __m256i);
        let shifted = _mm256_srli_epi64::<47>(acc_vec);
        let mut data = _mm256_xor_si256(acc_vec, shifted);
        let key = _mm256_loadu_si256(secret.add(8 * i) as *const __m256i);
        data = _mm256_xor_si256(data, key);
        let lo = _mm256_mul_epu32(data, prime);
        let hi = _mm256_mul_epu32(_mm256_srli_epi64::<32>(data), prime);
        let prod = _mm256_add_epi64(lo, _mm256_slli_epi64::<32>(hi));
        _mm256_storeu_si256(acc.add(i) as *mut __m256i, prod);
        i += 4;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn xxh3_accumulate_avx2(
    acc: *mut u64,
    input: *const u8,
    secret: *const u8,
    nb_stripes: usize,
) {
    for n in 0..nb_stripes {
        xxh3_accumulate_512_avx2(
            acc,
            input.add(n * XXH_STRIPE_LEN),
            secret.add(n * XXH_SECRET_CONSUME_RATE),
        );
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn xxh3_hash_long_64b_avx2(
    input: *const u8,
    len: usize,
    secret: *const u8,
    secret_size: usize,
) -> u64 {
    let nb_stripes_per_block = (secret_size - XXH_STRIPE_LEN) / XXH_SECRET_CONSUME_RATE;
    let block_len = XXH_STRIPE_LEN * nb_stripes_per_block;
    let nb_blocks = (len - 1) / block_len;
    let mut acc: [u64; XXH_ACC_NB] = [
        PRIME32_3 as u64,
        PRIME64_1,
        PRIME64_2,
        PRIME64_3,
        PRIME64_4,
        PRIME32_2 as u64,
        PRIME64_5,
        PRIME32_1 as u64,
    ];

    for n in 0..nb_blocks {
        xxh3_accumulate_avx2(
            acc.as_mut_ptr(),
            input.add(n * block_len),
            secret,
            nb_stripes_per_block,
        );
        xxh3_scramble_acc_avx2(acc.as_mut_ptr(), secret.add(secret_size - XXH_STRIPE_LEN));
    }

    let nb_stripes = (len - 1 - block_len * nb_blocks) / XXH_STRIPE_LEN;
    xxh3_accumulate_avx2(
        acc.as_mut_ptr(),
        input.add(nb_blocks * block_len),
        secret,
        nb_stripes,
    );
    xxh3_accumulate_512_avx2(
        acc.as_mut_ptr(),
        input.add(len - XXH_STRIPE_LEN),
        secret.add(secret_size - XXH_STRIPE_LEN - 7),
    );

    xxh3_merge_accs(
        acc.as_ptr(),
        secret.add(11),
        (len as u64).wrapping_mul(PRIME64_1),
    )
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn xxh3_hash_long_128b_avx2(
    input: *const u8,
    len: usize,
    secret: *const u8,
    secret_size: usize,
) -> Hash128 {
    let nb_stripes_per_block = (secret_size - XXH_STRIPE_LEN) / XXH_SECRET_CONSUME_RATE;
    let block_len = XXH_STRIPE_LEN * nb_stripes_per_block;
    let nb_blocks = (len - 1) / block_len;
    let mut acc: [u64; XXH_ACC_NB] = [
        PRIME32_3 as u64,
        PRIME64_1,
        PRIME64_2,
        PRIME64_3,
        PRIME64_4,
        PRIME32_2 as u64,
        PRIME64_5,
        PRIME32_1 as u64,
    ];

    for n in 0..nb_blocks {
        xxh3_accumulate_avx2(
            acc.as_mut_ptr(),
            input.add(n * block_len),
            secret,
            nb_stripes_per_block,
        );
        xxh3_scramble_acc_avx2(acc.as_mut_ptr(), secret.add(secret_size - XXH_STRIPE_LEN));
    }

    let nb_stripes = (len - 1 - block_len * nb_blocks) / XXH_STRIPE_LEN;
    xxh3_accumulate_avx2(
        acc.as_mut_ptr(),
        input.add(nb_blocks * block_len),
        secret,
        nb_stripes,
    );
    xxh3_accumulate_512_avx2(
        acc.as_mut_ptr(),
        input.add(len - XXH_STRIPE_LEN),
        secret.add(secret_size - XXH_STRIPE_LEN - 7),
    );

    let low = xxh3_merge_accs(
        acc.as_ptr(),
        secret.add(11),
        (len as u64).wrapping_mul(PRIME64_1),
    );
    let high = xxh3_merge_accs(
        acc.as_ptr(),
        secret.add(secret_size - XXH_STRIPE_LEN - 11),
        !(len as u64).wrapping_mul(PRIME64_2),
    );
    Hash128 { low, high }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
#[inline]
unsafe fn xxh3_accumulate_512_neon(acc: *mut u64, input: *const u8, secret: *const u8) {
    use core::arch::aarch64::*;
    let mut i = 0;
    while i < XXH_ACC_NB / 2 {
        let data_vec_1 = vreinterpretq_u64_u8(vld1q_u8(input.add(i * 16)));
        let data_vec_2 = vreinterpretq_u64_u8(vld1q_u8(input.add((i + 1) * 16)));
        let key_vec_1 = vreinterpretq_u64_u8(vld1q_u8(secret.add(i * 16)));
        let key_vec_2 = vreinterpretq_u64_u8(vld1q_u8(secret.add((i + 1) * 16)));
        let data_swap_1 = vcombine_u64(vget_high_u64(data_vec_1), vget_low_u64(data_vec_1));
        let data_swap_2 = vcombine_u64(vget_high_u64(data_vec_2), vget_low_u64(data_vec_2));
        let data_key_1 = veorq_u64(data_vec_1, key_vec_1);
        let data_key_2 = veorq_u64(data_vec_2, key_vec_2);
        let data_key_lo = vuzp1q_u32(
            vreinterpretq_u32_u64(data_key_1),
            vreinterpretq_u32_u64(data_key_2),
        );
        let data_key_hi = vuzp2q_u32(
            vreinterpretq_u32_u64(data_key_1),
            vreinterpretq_u32_u64(data_key_2),
        );
        let sum_1 = vmlal_u32(
            data_swap_1,
            vget_low_u32(data_key_lo),
            vget_low_u32(data_key_hi),
        );
        let sum_2 = vmlal_u32(
            data_swap_2,
            vget_high_u32(data_key_lo),
            vget_high_u32(data_key_hi),
        );
        let acc_1 = vld1q_u64(acc.add(2 * i) as *const u64);
        let acc_2 = vld1q_u64(acc.add(2 * i + 2) as *const u64);
        vst1q_u64(acc.add(2 * i), vaddq_u64(acc_1, sum_1));
        vst1q_u64(acc.add(2 * i + 2), vaddq_u64(acc_2, sum_2));
        i += 2;
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
#[inline]
unsafe fn xxh3_scramble_acc_neon(acc: *mut u64, secret: *const u8) {
    use core::arch::aarch64::*;
    let prime_lo = vdup_n_u32(PRIME32_1 as u32);
    let prime_hi = vreinterpretq_u32_u64(vdupq_n_u64((PRIME32_1 as u64) << 32));
    let mut i = 0;
    while i < XXH_ACC_NB / 2 {
        let acc_vec = vld1q_u64(acc.add(2 * i) as *const u64);
        let shifted = vshrq_n_u64::<47>(acc_vec);
        let data_vec = veorq_u64(acc_vec, shifted);
        let key_vec = vreinterpretq_u64_u8(vld1q_u8(secret.add(i * 16)));
        let data_key = veorq_u64(data_vec, key_vec);
        let prod_hi = vmulq_u32(vreinterpretq_u32_u64(data_key), prime_hi);
        let data_key_lo = vmovn_u64(data_key);
        let result = vmlal_u32(vreinterpretq_u64_u32(prod_hi), data_key_lo, prime_lo);
        vst1q_u64(acc.add(2 * i), result);
        i += 1;
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
unsafe fn xxh3_accumulate_neon(
    acc: *mut u64,
    input: *const u8,
    secret: *const u8,
    nb_stripes: usize,
) {
    for n in 0..nb_stripes {
        xxh3_accumulate_512_neon(
            acc,
            input.add(n * XXH_STRIPE_LEN),
            secret.add(n * XXH_SECRET_CONSUME_RATE),
        );
    }
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
unsafe fn xxh3_hash_long_64b_neon(
    input: *const u8,
    len: usize,
    secret: *const u8,
    secret_size: usize,
) -> u64 {
    let nb_stripes_per_block = (secret_size - XXH_STRIPE_LEN) / XXH_SECRET_CONSUME_RATE;
    let block_len = XXH_STRIPE_LEN * nb_stripes_per_block;
    let nb_blocks = (len - 1) / block_len;
    let mut acc: [u64; XXH_ACC_NB] = [
        PRIME32_3 as u64,
        PRIME64_1,
        PRIME64_2,
        PRIME64_3,
        PRIME64_4,
        PRIME32_2 as u64,
        PRIME64_5,
        PRIME32_1 as u64,
    ];

    for n in 0..nb_blocks {
        xxh3_accumulate_neon(
            acc.as_mut_ptr(),
            input.add(n * block_len),
            secret,
            nb_stripes_per_block,
        );
        xxh3_scramble_acc_neon(acc.as_mut_ptr(), secret.add(secret_size - XXH_STRIPE_LEN));
    }

    let nb_stripes = (len - 1 - block_len * nb_blocks) / XXH_STRIPE_LEN;
    xxh3_accumulate_neon(
        acc.as_mut_ptr(),
        input.add(nb_blocks * block_len),
        secret,
        nb_stripes,
    );
    xxh3_accumulate_512_neon(
        acc.as_mut_ptr(),
        input.add(len - XXH_STRIPE_LEN),
        secret.add(secret_size - XXH_STRIPE_LEN - 7),
    );

    xxh3_merge_accs(
        acc.as_ptr(),
        secret.add(11),
        (len as u64).wrapping_mul(PRIME64_1),
    )
}

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
#[target_feature(enable = "neon")]
unsafe fn xxh3_hash_long_128b_neon(
    input: *const u8,
    len: usize,
    secret: *const u8,
    secret_size: usize,
) -> Hash128 {
    let nb_stripes_per_block = (secret_size - XXH_STRIPE_LEN) / XXH_SECRET_CONSUME_RATE;
    let block_len = XXH_STRIPE_LEN * nb_stripes_per_block;
    let nb_blocks = (len - 1) / block_len;
    let mut acc: [u64; XXH_ACC_NB] = [
        PRIME32_3 as u64,
        PRIME64_1,
        PRIME64_2,
        PRIME64_3,
        PRIME64_4,
        PRIME32_2 as u64,
        PRIME64_5,
        PRIME32_1 as u64,
    ];

    for n in 0..nb_blocks {
        xxh3_accumulate_neon(
            acc.as_mut_ptr(),
            input.add(n * block_len),
            secret,
            nb_stripes_per_block,
        );
        xxh3_scramble_acc_neon(acc.as_mut_ptr(), secret.add(secret_size - XXH_STRIPE_LEN));
    }

    let nb_stripes = (len - 1 - block_len * nb_blocks) / XXH_STRIPE_LEN;
    xxh3_accumulate_neon(
        acc.as_mut_ptr(),
        input.add(nb_blocks * block_len),
        secret,
        nb_stripes,
    );
    xxh3_accumulate_512_neon(
        acc.as_mut_ptr(),
        input.add(len - XXH_STRIPE_LEN),
        secret.add(secret_size - XXH_STRIPE_LEN - 7),
    );

    let low = xxh3_merge_accs(
        acc.as_ptr(),
        secret.add(11),
        (len as u64).wrapping_mul(PRIME64_1),
    );
    let high = xxh3_merge_accs(
        acc.as_ptr(),
        secret.add(secret_size - XXH_STRIPE_LEN - 11),
        !(len as u64).wrapping_mul(PRIME64_2),
    );
    Hash128 { low, high }
}

#[inline(always)]
fn xxh3_128bits_scalar(input: *const u8, len: usize) -> Hash128 {
    let secret = K_SECRET.as_ptr();
    if len <= 16 {
        return xxh3_len_0to16_128b(input, len, secret, 0);
    }
    if len <= 128 {
        return xxh3_len_17to128_128b(input, len, secret, 0);
    }
    if len <= XXH3_MIDSIZE_MAX {
        return xxh3_len_129to240_128b(input, len, secret, 0);
    }
    xxh3_hash_long_128b(input, len, secret, XXH_SECRET_DEFAULT_SIZE)
}

#[inline(always)]
pub(crate) fn xxh3_128bits(input: *const u8, len: usize) -> Hash128 {
    if len <= XXH3_MIDSIZE_MAX {
        return xxh3_128bits_scalar(input, len);
    }
    let secret = K_SECRET.as_ptr();
    #[cfg(target_arch = "x86_64")]
    if has_avx2() {
        return unsafe { xxh3_hash_long_128b_avx2(input, len, secret, XXH_SECRET_DEFAULT_SIZE) };
    }
    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    return unsafe { xxh3_hash_long_128b_neon(input, len, secret, XXH_SECRET_DEFAULT_SIZE) };
    #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
    xxh3_hash_long_128b(input, len, secret, XXH_SECRET_DEFAULT_SIZE)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_xxhash_xxh3_64bits(data: *const u8, len: usize) -> u64 {
    xxh3_64bits(data, len)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_xxhash_xxh3_128bits(data: *const u8, len: usize, out: *mut u64) {
    let h = xxh3_128bits(data, len);
    *out = h.low;
    *out.add(1) = h.high;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_xxhash_xxh3_64bits_scalar(data: *const u8, len: usize) -> u64 {
    xxh3_64bits_scalar(data, len)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn llvm_xxhash_xxh3_128bits_scalar(
    data: *const u8,
    len: usize,
    out: *mut u64,
) {
    let h = xxh3_128bits_scalar(data, len);
    *out = h.low;
    *out.add(1) = h.high;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash64(data: &[u8]) -> u64 {
        xxh3_64bits(data.as_ptr(), data.len())
    }

    #[test]
    fn golden_vectors() {
        let cases: &[(&[u8], u64)] = &[
            (b"", 0x2d06_8005_38d3_94c2),
            (b"a", 0xe6c6_32b6_1e96_4e1f),
            (b"abc", 0x78af_5f94_892f_3950),
            (b"message digest", 0x160d_8e93_29be_94f9),
            (
                b"The quick brown fox jumps over the lazy dog",
                0xce7d_19a5_418f_b365,
            ),
        ];
        for &(input, expected) in cases {
            assert_eq!(hash64(input), expected, "input {:?}", input);
        }
    }

    fn hash128(data: &[u8]) -> (u64, u64) {
        let h = xxh3_128bits(data.as_ptr(), data.len());
        (h.low, h.high)
    }

    #[test]
    fn golden_vectors_128() {
        let cases: &[(&[u8], u64, u64)] = &[
            (b"", 0x6001_c324_468d_497f, 0x99aa_06d3_0147_98d8),
            (b"a", 0xe6c6_32b6_1e96_4e1f, 0xa96f_af70_5af1_6834),
            (b"abc", 0x78af_5f94_892f_3950, 0x06b0_5ab6_733a_6185),
            (
                b"message digest",
                0x0abf_abec_b8e3_a424,
                0x34ab_715d_95e3_b649,
            ),
            (
                b"The quick brown fox jumps over the lazy dog",
                0x24a1_cc2e_3a8a_7651,
                0xddd6_5020_5ca3_e7fa,
            ),
        ];
        for &(input, low, high) in cases {
            assert_eq!(hash128(input), (low, high), "input {:?}", input);
        }
    }

    #[test]
    fn length_sweep_does_not_panic() {
        let data: Vec<u8> = (0..4096).map(|i| (i * 37 + 11) as u8).collect();
        for len in 0..=data.len() {
            let _ = hash64(&data[..len]);
            let _ = hash128(&data[..len]);
        }
    }

    fn check_backend_matches_scalar() {
        let data: Vec<u8> = (0..4096).map(|i| (i * 37 + 11) as u8).collect();
        for &len in &[241usize, 242, 255, 256, 511, 512, 768, 1024, 2048, 4096] {
            let scalar64 = xxh3_64bits_scalar(data.as_ptr(), len);
            let dispatched64 = xxh3_64bits(data.as_ptr(), len);
            assert_eq!(scalar64, dispatched64, "64-bit length {}", len);
            let scalar128 = xxh3_128bits_scalar(data.as_ptr(), len);
            let dispatched128 = xxh3_128bits(data.as_ptr(), len);
            assert_eq!(
                (scalar128.low, scalar128.high),
                (dispatched128.low, dispatched128.high),
                "128-bit length {}",
                len
            );
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_matches_scalar() {
        if !has_avx2() {
            return;
        }
        check_backend_matches_scalar();
    }

    #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
    #[test]
    fn neon_matches_scalar() {
        check_backend_matches_scalar();
    }
}

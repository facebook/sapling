/*
 * Portions Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/*
 * Copyright Matt Mackall <mpm@selenic.com> and others
 *
 * This software may be used and distributed according to the terms of
 * the GNU General Public License, incorporated herein by reference.
 */

#ifndef _HG_BITMANIPULATION_H_
#define _HG_BITMANIPULATION_H_

#include <string.h>

#include "compat.h"

#if defined(_MSC_VER)
/* Windows only supports little-endian platforms */
static inline uint64_t hg_be_u64(uint64_t x) {
  return _byteswap_uint64(x);
}
static inline uint32_t hg_be_u32(uint32_t x) {
  return _byteswap_ulong(x);
}
static inline uint16_t hg_be_u16(uint16_t x) {
  return _byteswap_ushort(x);
}
#elif __BYTE_ORDER__ == __ORDER_LITTLE_ENDIAN__
static inline uint64_t hg_be_u64(uint64_t x) {
  return __builtin_bswap64(x);
}
static inline uint32_t hg_be_u32(uint32_t x) {
  return __builtin_bswap32(x);
}
static inline uint16_t hg_be_u16(uint16_t x) {
  return __builtin_bswap16(x);
}
#else
/* For completeness... */
static inline uint64_t hg_be_u64(uint64_t x) {
  return x;
}
static inline uint32_t hg_be_u32(uint32_t x) {
  return x;
}
static inline uint16_t hg_be_u16(uint16_t x) {
  return x;
}
#endif

static inline uint32_t getbe32(const char* c) {
  uint32_t value;
  memcpy(&value, c, sizeof(value));
  return hg_be_u32(value);
}

static inline uint16_t getbeuint16(const char* c) {
  uint16_t value;
  memcpy(&value, c, sizeof(value));
  return hg_be_u16(value);
}

static inline int16_t getbeint16(const char* c) {
  /*
   * Note: this code technically has undefined behavior for negative
   * values, although it's written in a way that the compiler and UBSAN
   * hopefully shouldn't complain about it.
   *
   * This relies on the platform using 2s-compliment representations for
   * signed integers.  This isn't guaranteed by the C standard, but is
   * true in practice for all modern platforms.
   */
  union {
    uint16_t unsignedvalue;
    int16_t signedvalue;
  } u;
  u.unsignedvalue = getbeuint16(c);
  return u.signedvalue;
}

static inline void putbe32(uint32_t x, char* c) {
  uint32_t v = hg_be_u32(x);
  memcpy(c, &v, sizeof(v));
}

static inline double getbefloat64(const char* c) {
  uint64_t n;
  double d;
  memcpy(&n, c, sizeof(n));
  n = hg_be_u64(n);
  memcpy(&d, &n, sizeof(n));
  return d;
}

#endif

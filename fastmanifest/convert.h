// Copyright 2016-present Facebook. All Rights Reserved.
//
// convert.h: hex-string conversions
//
// no-check-code

#ifndef __FASTMANIFEST_CONVERT_H__
#define __FASTMANIFEST_CONVERT_H__

#include <stdbool.h>
#include <stdint.h>

#include "node.h"

static int8_t hextable[256] = {
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
     0,  1,  2,  3,  4,  5,  6,  7,  8,  9, -1, -1, -1, -1, -1, -1, /* 0-9 */
    -1, 10, 11, 12, 13, 14, 15, -1, -1, -1, -1, -1, -1, -1, -1, -1, /* A-F */
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, 10, 11, 12, 13, 14, 15, -1, -1, -1, -1, -1, -1, -1, -1, -1, /* a-f */
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1
};

static char chartable[16] = {
    '0', '1', '2', '3', '4', '5', '6', '7',
    '8', '9', 'a', 'b', 'c', 'd', 'e', 'f'
};

/*
 * Turn a hex-encoded string into binary.  Returns false on failure.
 */
static inline bool unhexlify(const char *input, int len, uint8_t *dst) {
  if (len != SHA1_BYTES * 2) {
    // wtf.
    return false;
  }

  for (size_t ix = 0; ix < len; ix += 2, dst++) {
    int hi = hextable[(unsigned char) input[ix]];
    int lo = hextable[(unsigned char) input[ix + 1]];

    if (hi < 0 || lo < 0) {
      return false;
    }
    *dst = (hi << 4) | lo;
  }

  return true;
}

/*
 * Turn binary data into a hex-encoded string.
 */
static inline void hexlify(const uint8_t *input, int len, char *dst) {
  for (size_t ix = 0; ix < len; ix++, dst += 2) {
    unsigned char ch = (unsigned char) input[ix];
    char hi = chartable[ch >> 4];
    char lo = chartable[ch & 0xf];

    *dst = hi;
    *(dst + 1) = lo;
  }
}


#endif /* #ifndef __FASTMANIFEST_CONVERT_H__ */

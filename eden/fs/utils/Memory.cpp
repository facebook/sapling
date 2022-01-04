/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/Memory.h"

#include <stdio.h>
#include <stdlib.h>

namespace facebook::eden {

void assertZeroBits(const void* memory, size_t size) {
  if (0 == size) {
    return;
  }
  auto* p = static_cast<const unsigned char*>(memory);
  if (p[0] || memcmp(p, p + 1, size - 1)) {
    fprintf(stderr, "unexpected nonzero bits: ");
    for (size_t i = 0; i < size; ++i) {
      fprintf(stderr, "%01x%01x", p[i] & 15, p[i] >> 4);
    }
    fprintf(stderr, "\n");
    fflush(stderr);
    abort();
  }
}
} // namespace facebook::eden

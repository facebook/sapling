// Copyright 2016-present Facebook. All Rights Reserved.
//
// bsearch.c: binary search implementation with context-aware callback.
//
// no-check-code

#include <stddef.h>
#include <stdio.h>

#include "bsearch.h"

size_t bsearch_between(
    const void* needle,
    const void* base,
    const size_t nel,
    const size_t width,
    int (*compare)(
        const void* needle,
        const void* fromarray,
        const void* context),
    const void* context) {
  ptrdiff_t start = 0;
  ptrdiff_t end = nel;

  while (start < end) {
    ptrdiff_t midpoint = start + ((end - start) / 2);

    if (midpoint == nel) {
      return nel;
    }

    const void* ptr = (const void*)((char*)base + (midpoint * width));

    int cmp = compare(needle, ptr, context);

    if (cmp == 0) {
      return midpoint;
    } else if (cmp < 0) {
      end = midpoint;
    } else {
      start = midpoint + 1;
    }
  }

  return start;
}

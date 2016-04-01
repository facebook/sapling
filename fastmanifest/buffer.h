// Copyright 2016-present Facebook. All Rights Reserved.
//
// buffer.c: declarations for a generic mechanism to expand a heap-allocated
//           buffer.  this is for internal use only.
//
// no-check-code

#ifndef __FASTMANIFEST_BUFFER_H__
#define __FASTMANIFEST_BUFFER_H__

#include <stdbool.h>
#include <stddef.h>

static inline bool expand_to_fit(
    char **buffer, size_t *buffer_idx, size_t *buffer_sz,
    size_t input_sz,
    const float factor,
    const size_t min_increment,
    const size_t max_increment) {
  size_t remaining = *buffer_sz - *buffer_idx;
  if (input_sz > remaining) {
    // need realloc
    size_t new_sz = factor * ((float) *buffer_sz);
    if (new_sz < min_increment + *buffer_sz) {
      new_sz = min_increment + *buffer_sz;
    }
    if (new_sz > max_increment + *buffer_sz) {
      new_sz = max_increment + *buffer_sz;
    }
    if (new_sz < input_sz + *buffer_sz) {
      new_sz = input_sz + *buffer_sz;
    }

    void* newbuffer = realloc(*buffer, new_sz);
    if (newbuffer == NULL) {
      return false;
    }

    *buffer = newbuffer;
    *buffer_sz = new_sz;
  }

  return true;
}

extern bool buffer_append(
    char **buffer, size_t *buffer_idx, size_t *buffer_sz,
    char *input, size_t input_sz,
    const float factor,
    const size_t min_increment,
    const size_t max_increment);

#endif /* __FASTMANIFEST_BUFFER_H__ */

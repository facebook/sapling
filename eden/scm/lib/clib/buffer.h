// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// buffer.c: declarations for a generic mechanism to expand a heap-allocated
//           buffer.  this is for internal use only.
// no-check-code

#ifndef FBHGEXT_CLIB_BUFFER_H
#define FBHGEXT_CLIB_BUFFER_H

#include <stdbool.h>
#include <stddef.h>
#include <stdlib.h>

static inline bool expand_to_fit(
    void** buffer,
    size_t num_slots_used,
    size_t* num_slots_total,
    size_t input_count,
    size_t item_sz,
    const float factor,
    const size_t min_increment,
    const size_t max_increment) {
  size_t remaining = *num_slots_total - num_slots_used;
  if (input_count > remaining) {
    // need realloc
    size_t new_slots_total = factor * ((float)*num_slots_total);
    if (new_slots_total < min_increment + *num_slots_total) {
      new_slots_total = min_increment + *num_slots_total;
    }
    if (new_slots_total > max_increment + *num_slots_total) {
      new_slots_total = max_increment + *num_slots_total;
    }
    if (new_slots_total < input_count + *num_slots_total) {
      new_slots_total = input_count + *num_slots_total;
    }

    void* newbuffer = realloc(*buffer, item_sz * new_slots_total);
    if (newbuffer == NULL) {
      return false;
    }

    *buffer = newbuffer;
    *num_slots_total = new_slots_total;
  }

  return true;
}

extern bool buffer_append(
    char** buffer,
    size_t* buffer_idx,
    size_t* buffer_sz,
    char* input,
    size_t input_sz,
    const float factor,
    const size_t min_increment,
    const size_t max_increment);

#endif /* FBHGEXT_CLIB_BUFFER_H */

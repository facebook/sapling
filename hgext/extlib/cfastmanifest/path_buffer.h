// Copyright 2016-present Facebook. All Rights Reserved.
//
// path_buffer.h: macros for managing a path buffer.
//
// no-check-code

#ifndef CFASTMANIFEST_PATH_BUFFER_H
#define CFASTMANIFEST_PATH_BUFFER_H

#include "lib/clib/buffer.h"

// a common usage pattern for this module is to store a path.  the path can
// be of any length, theoretically, so we have to support expansion.
#define DEFAULT_PATH_BUFFER_SZ 16384
#define PATH_BUFFER_GROWTH_FACTOR 1.2
#define PATH_BUFFER_MINIMUM_GROWTH 65536
#define PATH_BUFFER_MAXIMUM_GROWTH (1024 * 1024)

#define PATH_APPEND(buffer, buffer_idx, buffer_sz, input, input_sz) \
  buffer_append(                                                    \
      buffer,                                                       \
      buffer_idx,                                                   \
      buffer_sz,                                                    \
      input,                                                        \
      input_sz,                                                     \
      PATH_BUFFER_GROWTH_FACTOR,                                    \
      PATH_BUFFER_MINIMUM_GROWTH,                                   \
      PATH_BUFFER_MAXIMUM_GROWTH)

#endif // CFASTMANIFEST_PATH_BUFFER_H

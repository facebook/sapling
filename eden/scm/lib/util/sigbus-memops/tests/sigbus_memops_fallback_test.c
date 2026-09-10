/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#include "sigbus_memops.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(expression)                                 \
  do {                                                    \
    if (!(expression)) {                                  \
      fprintf(stderr, "check failed: %s\n", #expression); \
      exit(1);                                            \
    }                                                     \
  } while (0)

int main(void) {
  const uint8_t source[] = {1, 2, 3, 4, 5, 6, 7, 8};
  uint8_t destination[sizeof(source)] = {0};

  CHECK(!sigbus_is_protected());
  CHECK(sigbus_install_handler() == 0);
#ifndef _WIN32
  CHECK(!sigbus_try_handle(SIGBUS, NULL, NULL));
#endif
  CHECK(sigbus_try_memcpy(destination, source, sizeof(source)));
  CHECK(memcmp(destination, source, sizeof(source)) == 0);
  CHECK(sigbus_try_read(source, sizeof(source)));
  CHECK(sigbus_try_read(NULL, 0));
  return 0;
}

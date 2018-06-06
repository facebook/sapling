// Copyright 2016-present Facebook. All Rights Reserved.
//
// tests.h: convenience functions for unit tests.
//
// no-check-code

#ifndef __TESTLIB_TESTS_H__
#define __TESTLIB_TESTS_H__

#include <stdio.h>
#include <stdlib.h>

#include "hgext/extlib/cfastmanifest/tree.h"
#include "lib/clib/portability/portability.h"
#include "node.h"
#include "result.h"

#define ASSERT(cond)                         \
  if (!(cond)) {                             \
    printf("failed on line %d\n", __LINE__); \
    exit(37);                                \
  }

#define STRPLUSLEN(__str__) __str__, strlen(__str__)

typedef struct _get_path_unfiltered_result_t {
  get_path_code_t code;
  const node_t* node;
} get_path_unfiltered_result_t;

extern get_path_unfiltered_result_t
get_path_unfiltered(tree_t* const tree, const char* path, const size_t path_sz);

/**
 * Computes a hash based on a value.  It's not a great checksum, but it's enough
 * for basic tests.
 */
static inline uint8_t* int2sha1hash(uint32_t value, uint8_t* sha1hash) {
  for (size_t ix = 0; ix < SHA1_BYTES; ix += sizeof(value), value++) {
    size_t left = SHA1_BYTES - ix;
    size_t bytes_to_copy = left > sizeof(value) ? sizeof(value) : left;
    memcpy(&sha1hash[ix], &value, bytes_to_copy);
  }
  return sha1hash;
}

typedef struct {
  char* path;
  size_t path_sz;
  uint32_t checksum_seed;
  uint8_t flags;
} add_to_tree_t;

/**
 * Adds a bunch of paths to a tree.
 */
static inline void
add_to_tree(tree_t* tree, add_to_tree_t* requests, size_t request_sz) {
  uint8_t buffer[CHECKSUM_BYTES];

  for (size_t ix = 0; ix < request_sz; ix++) {
    add_to_tree_t* request = &requests[ix];
    add_update_path_result_t result = add_or_update_path(
        tree,
        request->path,
        request->path_sz,
        int2sha1hash(request->checksum_seed, buffer),
        SHA1_BYTES,
        request->flags);
    ASSERT(result == ADD_UPDATE_PATH_OK);
  }
}

#endif /* #ifndef __TESTLIB_TESTS_H__ */

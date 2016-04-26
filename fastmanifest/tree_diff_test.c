// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_diff_test.c: tests to verify tree_diff
//
// no-check-code

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "node.h"
#include "tree.h"
#include "tests.h"

typedef struct {
  const char *path;
  const size_t path_sz;
  const bool left_present;
  uint32_t left_checksum_seed;
  const uint8_t left_flags;
  const bool right_present;
  uint32_t right_checksum_seed;
  const uint8_t right_flags;
} diff_expectation_t;

typedef struct {
  diff_expectation_t *expectations;
  size_t expectations_idx;
  size_t expectations_sz;
} diff_expectations_t;

static void expectations_matcher(
    const char *path,
    const size_t path_sz,
    const bool left_present,
    const uint8_t *left_checksum,
    const uint8_t left_checksum_sz,
    const uint8_t left_flags,
    const bool right_present,
    const uint8_t *right_checksum,
    const uint8_t right_checksum_sz,
    const uint8_t right_flags,
    void *context) {
  uint8_t buffer[CHECKSUM_BYTES];

  diff_expectations_t *expectations = (diff_expectations_t *) context;
  ASSERT(expectations->expectations_idx < expectations->expectations_sz);
  diff_expectation_t *expectation = &expectations->expectations[
      expectations->expectations_idx];

  ASSERT(expectation->path_sz == path_sz);
  ASSERT(memcmp(expectation->path, path, path_sz) == 0);
  ASSERT(expectation->left_present == left_present);
  if (left_present) {
    ASSERT(SHA1_BYTES == left_checksum_sz);
    ASSERT(memcmp(int2sha1hash(expectation->left_checksum_seed, buffer),
        left_checksum, left_checksum_sz) == 0);
    ASSERT(expectation->left_flags == left_flags);
  }
  ASSERT(expectation->right_present == right_present);
  if (right_present) {
    ASSERT(SHA1_BYTES == right_checksum_sz);
    ASSERT(memcmp(int2sha1hash(expectation->right_checksum_seed, buffer),
        right_checksum, right_checksum_sz) == 0);
    ASSERT(expectation->right_flags == right_flags);
  }

  expectations->expectations_idx++;
}

static void diff_empty_trees() {
  tree_t *left = alloc_tree();
  tree_t *right = alloc_tree();

  diff_expectation_t expectation_array[] =
      {
      };
  diff_expectations_t expectations = {
      expectation_array,
      0, sizeof(expectation_array) / sizeof(diff_expectation_t)
  };

  ASSERT(diff_trees(left, right, false, expectations_matcher, &expectations) ==
         DIFF_OK);
  ASSERT(expectations.expectations_idx == expectations.expectations_sz);

  ASSERT(diff_trees(left, right, true, expectations_matcher, &expectations) ==
         DIFF_OK);
  ASSERT(expectations.expectations_idx == expectations.expectations_sz);
}

/**
 * Diff two identical trees.
 */
static void diff_identical_trees() {
  tree_t *left = alloc_tree();
  tree_t *right = alloc_tree();

  add_to_tree_t toadd[] = {
      {STRPLUSLEN("abc"), 12345, 5},
      {STRPLUSLEN("ab/cdef/ghi"), 44252, 22},
      {STRPLUSLEN("ab/cdef/g/hi"), 112123, 64},
      {STRPLUSLEN("ab/cdef/g/hij"), 54654, 58},
      {STRPLUSLEN("ab/cdef/gh/ijk"), 45645105, 65},
      {STRPLUSLEN("ab/cdef/gh/i"), 5464154, 4},
  };

  add_to_tree(left, toadd, sizeof(toadd) / sizeof(add_to_tree_t));
  add_to_tree(right, toadd, sizeof(toadd) / sizeof(add_to_tree_t));

  diff_expectation_t normal_expectation_array[] =
      {
      };
  diff_expectations_t normal_expectations = {
      normal_expectation_array,
      0, sizeof(normal_expectation_array) / sizeof(diff_expectation_t)
  };

  ASSERT(diff_trees(
      left, right,
      false,
      expectations_matcher, &normal_expectations) == DIFF_OK);
  ASSERT(normal_expectations.expectations_idx ==
      normal_expectations.expectations_sz);

  diff_expectation_t include_all_expectation_array[] = {
      {
          STRPLUSLEN("ab/cdef/g/hi"),
          true, 112123, 64,
          true, 112123, 64,
      },
      {
          STRPLUSLEN("ab/cdef/g/hij"),
          true, 54654, 58,
          true, 54654, 58,
      },
      {
          STRPLUSLEN("ab/cdef/gh/i"),
          true, 5464154, 4,
          true, 5464154, 4,
      },
      {
          STRPLUSLEN("ab/cdef/gh/ijk"),
          true, 45645105, 65,
          true, 45645105, 65,
      },
      {
          STRPLUSLEN("ab/cdef/ghi"),
          true, 44252, 22,
          true, 44252, 22,
      },
      {
          STRPLUSLEN("abc"),
          true, 12345, 5,
          true, 12345, 5,
      },
  };
  diff_expectations_t include_all_expectations = {
      include_all_expectation_array,
      0, sizeof(include_all_expectation_array) / sizeof(diff_expectation_t)
  };

  ASSERT(diff_trees(
      left, right,
      true,
      expectations_matcher, &include_all_expectations) == DIFF_OK);
  ASSERT(include_all_expectations.expectations_idx ==
         include_all_expectations.expectations_sz);
}

int main(int argc, char *argv[]) {
  diff_empty_trees();
  diff_identical_trees();

  return 0;
}

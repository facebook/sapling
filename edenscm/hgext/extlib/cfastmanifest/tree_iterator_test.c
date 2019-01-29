// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_iterator_test.c: tests for traversing all the nodes of a tree in-order.
//
// no-check-code

#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

#include "edenscm/hgext/extlib/cfastmanifest/tree.h"
#include "tests.h"
#include "tree_iterator.h"

typedef struct _iterator_expectations_t {
  char* path;
  size_t path_sz;
  bool path_present;
  uint32_t checksum_primer;
  uint8_t flags;
} iterator_expectations_t;

static bool match_expectations(
    iterator_t* iterator,
    iterator_expectations_t* expectations,
    size_t expectations_sz) {
  size_t ix = 0;
  uint8_t expected_checksum[SHA1_BYTES];

  while (true) {
    iterator_result_t result = iterator_next(iterator);

    if (result.valid == false) {
      break;
    }

    if (ix >= expectations_sz) {
      return false;
    }

    iterator_expectations_t* expectation = &expectations[ix];
    ix++;

    if (expectation->path_present &&
        (expectation->path_sz != result.path_sz ||
         memcmp(expectation->path, result.path, expectation->path_sz) != 0)) {
      return false;
    }

    // prime the expected checksum
    int2sha1hash(expectation->checksum_primer, expected_checksum);

    if (SHA1_BYTES != result.checksum_sz ||
        memcmp(expected_checksum, result.checksum, SHA1_BYTES) != 0) {
      return false;
    }
  }

  return (ix == expectations_sz);
}

void test_empty_tree() {
  tree_t* tree = alloc_tree();
  iterator_t* iterator = create_iterator(tree, false);
  iterator_expectations_t expectations[] = {};

  ASSERT(match_expectations(
      iterator,
      expectations,
      sizeof(expectations) / sizeof(iterator_expectations_t)));

  destroy_iterator(iterator);
  destroy_tree(tree);
}

void test_simple_tree() {
  tree_t* tree = alloc_tree();

  add_to_tree_t toadd[] = {
      {STRPLUSLEN("abc"), 12345, 5},
  };

  add_to_tree(tree, toadd, sizeof(toadd) / sizeof(add_to_tree_t));

  iterator_t* iterator = create_iterator(tree, true);
  iterator_expectations_t expectations[] = {
      {STRPLUSLEN("abc"), true, 12345, 5}};

  ASSERT(match_expectations(
      iterator,
      expectations,
      sizeof(expectations) / sizeof(iterator_expectations_t)));

  destroy_iterator(iterator);
  destroy_tree(tree);
}

void test_complicated_tree() {
  tree_t* tree = alloc_tree();

  add_to_tree_t toadd[] = {
      {STRPLUSLEN("abc"), 12345, 5},
      {STRPLUSLEN("ab/cdef/gh"), 64342, 55},
      {STRPLUSLEN("ab/cdef/ghi/jkl"), 51545, 57},
      {STRPLUSLEN("ab/cdef/ghi/jklm"), 54774, 12},
      {STRPLUSLEN("ab/cdef/ghi/jklmn"), 48477, 252},
      {STRPLUSLEN("a"), 577, 14},
  };

  add_to_tree(tree, toadd, sizeof(toadd) / sizeof(add_to_tree_t));

  iterator_t* iterator = create_iterator(tree, true);
  iterator_expectations_t expectations[] = {
      {STRPLUSLEN("a"), true, 577, 14},
      {STRPLUSLEN("ab/cdef/gh"), true, 64342, 55},
      {STRPLUSLEN("ab/cdef/ghi/jkl"), true, 51545, 57},
      {STRPLUSLEN("ab/cdef/ghi/jklm"), true, 54774, 12},
      {STRPLUSLEN("ab/cdef/ghi/jklmn"), true, 48477, 252},
      {STRPLUSLEN("abc"), true, 12345, 5},
  };

  ASSERT(match_expectations(
      iterator,
      expectations,
      sizeof(expectations) / sizeof(iterator_expectations_t)));

  destroy_iterator(iterator);
}

int main(int argc, char* argv[]) {
  test_empty_tree();
  test_simple_tree();
  test_complicated_tree();
  return 0;
}

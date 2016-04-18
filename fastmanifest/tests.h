// Copyright 2016-present Facebook. All Rights Reserved.
//
// tests.h: convenience functions for unit tests.
//
// no-check-code

#ifndef __TESTLIB_TESTS_H__
#define __TESTLIB_TESTS_H__

#include <stdio.h>
#include <stdlib.h>

#include "node.h"
#include "result.h"
#include "tree.h"

#define ASSERT(cond) if (!(cond)) {             \
    printf("failed on line %d\n", __LINE__);    \
    exit(37);                                   \
  }

#define STRPLUSLEN(__str__) __str__, strlen(__str__)

typedef struct _get_path_unfiltered_result_t {
  get_path_code_t code;
  const node_t *node;
} get_path_unfiltered_result_t;

extern get_path_unfiltered_result_t get_path_unfiltered(
    tree_t *const tree,
    const char *path,
    const size_t path_sz);

#endif /* #ifndef __TESTLIB_TESTS_H__ */

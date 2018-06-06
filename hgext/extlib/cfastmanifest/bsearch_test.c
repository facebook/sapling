// Copyright 2016-present Facebook. All Rights Reserved.
//
// bsearch_test.c: tests for binary search with a context-aware callback.
//
// no-check-code

#include "bsearch.h"
#include "tests.h"

#define CMP(left, right) ((int)(*((intptr_t*)left) - *((intptr_t*)right)))

COMPARATOR_BUILDER(intptr_cmp, CMP)

#define BSEARCH_TEST(needle, expected, ...)                   \
  {                                                           \
    size_t result;                                            \
    intptr_t _needle = needle;                                \
    intptr_t* array = (intptr_t[]){__VA_ARGS__};              \
                                                              \
    result = bsearch_between(                                 \
        &_needle,                                             \
        array,                                                \
        sizeof((intptr_t[]){__VA_ARGS__}) / sizeof(intptr_t), \
        sizeof(intptr_t),                                     \
        &intptr_cmp,                                          \
        NULL);                                                \
    ASSERT(result == expected);                               \
  }

void test_bsearch() {
  BSEARCH_TEST(20, 1, 18, 21);

  BSEARCH_TEST(20, 2, 15, 18, 21, );

  BSEARCH_TEST(20, 2, 15, 18, 20, 21, );

  BSEARCH_TEST(10, 0, 15, 18, 20, 21, );

  BSEARCH_TEST(30, 4, 15, 18, 20, 21, );
}

int main(int argc, char* argv[]) {
  test_bsearch();

  return 0;
}

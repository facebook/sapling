// Copyright 2016-present Facebook. All Rights Reserved.
//
// tests.h: convenience macros for unit tests.
//
// no-check-code

#ifndef __TESTLIB_TESTS_H__
#define __TESTLIB_TESTS_H__

#include <stdio.h>
#include <stdlib.h>

#define ASSERT(cond) if (!(cond)) {             \
    printf("failed on line %d\n", __LINE__);    \
    exit(37);                                   \
  }

#define STRPLUSLEN(__str__) __str__, strlen(__str__)

#endif /* #ifndef __TESTLIB_TESTS_H__ */

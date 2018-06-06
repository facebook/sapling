// Copyright 2016-present Facebook. All Rights Reserved.
//
// bsearch.h: binary search declarations with context-aware callback.  this
//            is a standalone library.
//
// no-check-code

#ifndef __BSEARCH_BSEARCH_H__
#define __BSEARCH_BSEARCH_H__

#include <stdbool.h>
#include <stddef.h>
#include <sys/types.h>

/**
 * A generic binary search that allows a comparator to evaluate the placement of
 * a needle relative to its possible neighbors.
 *
 * Returns a value from 0 to nel, representing where a needle
 *
 * The comparator should return:
 *   <0 if the element should be placed before `left`.
 *   =0 if the element should be placed between `left` and `right`.
 *   >0 if the element should be placed after `right`.
 */
extern size_t bsearch_between(
    const void* needle,
    const void* base,
    const size_t nel,
    const size_t width,
    int (*compare)(
        const void* needle,
        const void* fromarray,
        const void* context),
    const void* context);

/**
 * A convenient macro to build comparators for `bsearch_between`.  Callers
 * should provide a LEFT_COMPARE, which is used to compare the left neighbor and
 * the needle, and RIGHT_COMPARE, which is used to compare the needle and the
 * right neighbor.
 *
 * Each comparator will be passed two void pointers and a context object.  It is
 * the responsibility of the caller to ensure that it can properly cast the
 * values to sane pointers.
 */

#define COMPARATOR_BUILDER(COMPARATOR_NAME, COMPARE)                    \
  int COMPARATOR_NAME(                                                  \
      const void* needle, const void* fromarray, const void* context) { \
    return COMPARE(needle, fromarray);                                  \
  }

#define CONTEXTUAL_COMPARATOR_BUILDER(COMPARATOR_NAME, COMPARE)         \
  int COMPARATOR_NAME(                                                  \
      const void* needle, const void* fromarray, const void* context) { \
    return COMPARE(needle, fromarray, context);                         \
  }

#endif /* #ifndef __BSEARCH_BSEARCH_H__ */

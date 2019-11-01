/*
 * Portions Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/*
 * Copyright Matt Mackall <mpm@selenic.com> and others
 *
 * This software may be used and distributed according to the terms of
 * the GNU General Public License, incorporated herein by reference.
 */

#ifndef _HG_BDIFF_H_
#define _HG_BDIFF_H_

#include "compat.h"

struct bdiff_line {
  int hash, n, e;
  ssize_t len;
  const char* l;
};

struct bdiff_hunk;
struct bdiff_hunk {
  int a1, a2, b1, b2;
  struct bdiff_hunk* next;
};

int bdiff_splitlines(const char* a, ssize_t len, struct bdiff_line** lr);
int bdiff_diff(
    struct bdiff_line* a,
    int an,
    struct bdiff_line* b,
    int bn,
    struct bdiff_hunk* base);
void bdiff_freehunks(struct bdiff_hunk* l);

#endif

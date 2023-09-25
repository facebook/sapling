# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright 2016 Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os

import cffi


ffi = cffi.FFI()
mpatch_c = os.path.join(os.path.join(os.path.dirname(__file__), "..", "mpatch.c"))
with open(mpatch_c) as f:
    ffi.set_source("cffi._mpatch", f.read(), include_dirs=["."])
ffi.cdef(
    """

struct mpatch_frag {
       int start, end, len;
       const char *data;
};

struct mpatch_flist {
       struct mpatch_frag *base, *head, *tail;
};

extern "Python" struct mpatch_flist* cffi_get_next_item(void*, ssize_t);

int mpatch_decode(const char *bin, ssize_t len, struct mpatch_flist** res);
ssize_t mpatch_calcsize(size_t len, struct mpatch_flist *l);
void mpatch_lfree(struct mpatch_flist *a);
static int mpatch_apply(char *buf, const char *orig, size_t len,
                        struct mpatch_flist *l);
struct mpatch_flist *mpatch_fold(void *bins,
                       struct mpatch_flist* (*get_next_item)(void*, ssize_t),
                       ssize_t start, ssize_t end);
"""
)

if __name__ == "__main__":
    ffi.compile()

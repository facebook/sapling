# Copyright (c) 2016-present, Gregory Szorc
# All rights reserved.
#
# This software may be modified and distributed under the terms
# of the BSD license. See the LICENSE file for details.

from __future__ import absolute_import

import cffi
import os


HERE = os.path.abspath(os.path.dirname(__file__))

SOURCES = ['zstd/%s' % p for p in (
    'common/entropy_common.c',
    'common/error_private.c',
    'common/fse_decompress.c',
    'common/xxhash.c',
    'common/zstd_common.c',
    'compress/fse_compress.c',
    'compress/huf_compress.c',
    'compress/zbuff_compress.c',
    'compress/zstd_compress.c',
    'decompress/huf_decompress.c',
    'decompress/zbuff_decompress.c',
    'decompress/zstd_decompress.c',
    'dictBuilder/divsufsort.c',
    'dictBuilder/zdict.c',
)]

INCLUDE_DIRS = [os.path.join(HERE, d) for d in (
    'zstd',
    'zstd/common',
    'zstd/compress',
    'zstd/decompress',
    'zstd/dictBuilder',
)]

with open(os.path.join(HERE, 'zstd', 'zstd.h'), 'rb') as fh:
    zstd_h = fh.read()

ffi = cffi.FFI()
ffi.set_source('_zstd_cffi', '''
/* needed for typedefs like U32 references in zstd.h */
#include "mem.h"
#define ZSTD_STATIC_LINKING_ONLY
#include "zstd.h"
''',
    sources=SOURCES, include_dirs=INCLUDE_DIRS)

# Rather than define the API definitions from zstd.h inline, munge the
# source in a way that cdef() will accept.
lines = zstd_h.splitlines()
lines = [l.rstrip() for l in lines if l.strip()]

# Strip preprocessor directives - they aren't important for our needs.
lines = [l for l in lines
         if not l.startswith((b'#if', b'#else', b'#endif', b'#include'))]

# Remove extern C block
lines = [l for l in lines if l not in (b'extern "C" {', b'}')]

# The version #defines don't parse and aren't necessary. Strip them.
lines = [l for l in lines if not l.startswith((
    b'#define ZSTD_H_235446',
    b'#define ZSTD_LIB_VERSION',
    b'#define ZSTD_QUOTE',
    b'#define ZSTD_EXPAND_AND_QUOTE',
    b'#define ZSTD_VERSION_STRING',
    b'#define ZSTD_VERSION_NUMBER'))]

# The C parser also doesn't like some constant defines referencing
# other constants.
# TODO we pick the 64-bit constants here. We should assert somewhere
# we're compiling for 64-bit.
def fix_constants(l):
    if l.startswith(b'#define ZSTD_WINDOWLOG_MAX '):
        return b'#define ZSTD_WINDOWLOG_MAX 27'
    elif l.startswith(b'#define ZSTD_CHAINLOG_MAX '):
        return b'#define ZSTD_CHAINLOG_MAX 28'
    elif l.startswith(b'#define ZSTD_HASHLOG_MAX '):
        return b'#define ZSTD_HASHLOG_MAX 27'
    elif l.startswith(b'#define ZSTD_CHAINLOG_MAX '):
        return b'#define ZSTD_CHAINLOG_MAX 28'
    elif l.startswith(b'#define ZSTD_CHAINLOG_MIN '):
        return b'#define ZSTD_CHAINLOG_MIN 6'
    elif l.startswith(b'#define ZSTD_SEARCHLOG_MAX '):
        return b'#define ZSTD_SEARCHLOG_MAX 26'
    elif l.startswith(b'#define ZSTD_BLOCKSIZE_ABSOLUTEMAX '):
        return b'#define ZSTD_BLOCKSIZE_ABSOLUTEMAX 131072'
    else:
        return l
lines = map(fix_constants, lines)

# ZSTDLIB_API isn't handled correctly. Strip it.
lines = [l for l in lines if not l.startswith(b'#  define ZSTDLIB_API')]
def strip_api(l):
    if l.startswith(b'ZSTDLIB_API '):
        return l[len(b'ZSTDLIB_API '):]
    else:
        return l
lines = map(strip_api, lines)

source = b'\n'.join(lines)
ffi.cdef(source.decode('latin1'))


if __name__ == '__main__':
    ffi.compile()

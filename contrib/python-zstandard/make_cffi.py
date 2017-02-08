# Copyright (c) 2016-present, Gregory Szorc
# All rights reserved.
#
# This software may be modified and distributed under the terms
# of the BSD license. See the LICENSE file for details.

from __future__ import absolute_import

import cffi
import distutils.ccompiler
import os
import re
import subprocess
import tempfile


HERE = os.path.abspath(os.path.dirname(__file__))

SOURCES = ['zstd/%s' % p for p in (
    'common/entropy_common.c',
    'common/error_private.c',
    'common/fse_decompress.c',
    'common/pool.c',
    'common/threading.c',
    'common/xxhash.c',
    'common/zstd_common.c',
    'compress/fse_compress.c',
    'compress/huf_compress.c',
    'compress/zstd_compress.c',
    'decompress/huf_decompress.c',
    'decompress/zstd_decompress.c',
    'dictBuilder/cover.c',
    'dictBuilder/divsufsort.c',
    'dictBuilder/zdict.c',
)]

HEADERS = [os.path.join(HERE, 'zstd', *p) for p in (
    ('zstd.h',),
    ('common', 'pool.h'),
    ('dictBuilder', 'zdict.h'),
)]

INCLUDE_DIRS = [os.path.join(HERE, d) for d in (
    'zstd',
    'zstd/common',
    'zstd/compress',
    'zstd/decompress',
    'zstd/dictBuilder',
)]

# cffi can't parse some of the primitives in zstd.h. So we invoke the
# preprocessor and feed its output into cffi.
compiler = distutils.ccompiler.new_compiler()

# Needed for MSVC.
if hasattr(compiler, 'initialize'):
    compiler.initialize()

# Distutils doesn't set compiler.preprocessor, so invoke the preprocessor
# manually.
if compiler.compiler_type == 'unix':
    args = list(compiler.executables['compiler'])
    args.extend([
        '-E',
        '-DZSTD_STATIC_LINKING_ONLY',
        '-DZDICT_STATIC_LINKING_ONLY',
    ])
elif compiler.compiler_type == 'msvc':
    args = [compiler.cc]
    args.extend([
        '/EP',
        '/DZSTD_STATIC_LINKING_ONLY',
        '/DZDICT_STATIC_LINKING_ONLY',
    ])
else:
    raise Exception('unsupported compiler type: %s' % compiler.compiler_type)

def preprocess(path):
    # zstd.h includes <stddef.h>, which is also included by cffi's boilerplate.
    # This can lead to duplicate declarations. So we strip this include from the
    # preprocessor invocation.
    with open(path, 'rb') as fh:
        lines = [l for l in fh if not l.startswith(b'#include <stddef.h>')]

    fd, input_file = tempfile.mkstemp(suffix='.h')
    os.write(fd, b''.join(lines))
    os.close(fd)

    try:
        process = subprocess.Popen(args + [input_file], stdout=subprocess.PIPE)
        output = process.communicate()[0]
        ret = process.poll()
        if ret:
            raise Exception('preprocessor exited with error')

        return output
    finally:
        os.unlink(input_file)


def normalize_output(output):
    lines = []
    for line in output.splitlines():
        # CFFI's parser doesn't like __attribute__ on UNIX compilers.
        if line.startswith(b'__attribute__ ((visibility ("default"))) '):
            line = line[len(b'__attribute__ ((visibility ("default"))) '):]

        if line.startswith(b'__attribute__((deprecated('):
            continue
        elif b'__declspec(deprecated(' in line:
            continue

        lines.append(line)

    return b'\n'.join(lines)


ffi = cffi.FFI()
ffi.set_source('_zstd_cffi', '''
#include "mem.h"
#define ZSTD_STATIC_LINKING_ONLY
#include "zstd.h"
#define ZDICT_STATIC_LINKING_ONLY
#include "pool.h"
#include "zdict.h"
''', sources=SOURCES, include_dirs=INCLUDE_DIRS)

DEFINE = re.compile(b'^\\#define ([a-zA-Z0-9_]+) ')

sources = []

for header in HEADERS:
    preprocessed = preprocess(header)
    sources.append(normalize_output(preprocessed))

    # Do another pass over source and find constants that were preprocessed
    # away.
    with open(header, 'rb') as fh:
        for line in fh:
            line = line.strip()
            m = DEFINE.match(line)
            if not m:
                continue

            # The parser doesn't like some constants with complex values.
            if m.group(1) in (b'ZSTD_LIB_VERSION', b'ZSTD_VERSION_STRING'):
                continue

            sources.append(m.group(0) + b' ...')

ffi.cdef(u'\n'.join(s.decode('latin1') for s in sources))

if __name__ == '__main__':
    ffi.compile()

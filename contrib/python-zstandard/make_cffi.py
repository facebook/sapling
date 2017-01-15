# Copyright (c) 2016-present, Gregory Szorc
# All rights reserved.
#
# This software may be modified and distributed under the terms
# of the BSD license. See the LICENSE file for details.

from __future__ import absolute_import

import cffi
import distutils.ccompiler
import os
import subprocess
import tempfile


HERE = os.path.abspath(os.path.dirname(__file__))

SOURCES = ['zstd/%s' % p for p in (
    'common/entropy_common.c',
    'common/error_private.c',
    'common/fse_decompress.c',
    'common/xxhash.c',
    'common/zstd_common.c',
    'compress/fse_compress.c',
    'compress/huf_compress.c',
    'compress/zstd_compress.c',
    'decompress/huf_decompress.c',
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
    ])
elif compiler.compiler_type == 'msvc':
    args = [compiler.cc]
    args.extend([
        '/EP',
        '/DZSTD_STATIC_LINKING_ONLY',
    ])
else:
    raise Exception('unsupported compiler type: %s' % compiler.compiler_type)

# zstd.h includes <stddef.h>, which is also included by cffi's boilerplate.
# This can lead to duplicate declarations. So we strip this include from the
# preprocessor invocation.

with open(os.path.join(HERE, 'zstd', 'zstd.h'), 'rb') as fh:
    lines = [l for l in fh if not l.startswith(b'#include <stddef.h>')]

fd, input_file = tempfile.mkstemp(suffix='.h')
os.write(fd, b''.join(lines))
os.close(fd)

args.append(input_file)

try:
    process = subprocess.Popen(args, stdout=subprocess.PIPE)
    output = process.communicate()[0]
    ret = process.poll()
    if ret:
        raise Exception('preprocessor exited with error')
finally:
    os.unlink(input_file)

def normalize_output():
    lines = []
    for line in output.splitlines():
        # CFFI's parser doesn't like __attribute__ on UNIX compilers.
        if line.startswith(b'__attribute__ ((visibility ("default"))) '):
            line = line[len(b'__attribute__ ((visibility ("default"))) '):]

        lines.append(line)

    return b'\n'.join(lines)

ffi = cffi.FFI()
ffi.set_source('_zstd_cffi', '''
#define ZSTD_STATIC_LINKING_ONLY
#include "zstd.h"
''', sources=SOURCES, include_dirs=INCLUDE_DIRS)

ffi.cdef(normalize_output().decode('latin1'))

if __name__ == '__main__':
    ffi.compile()

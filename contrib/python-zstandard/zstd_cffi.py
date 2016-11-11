# Copyright (c) 2016-present, Gregory Szorc
# All rights reserved.
#
# This software may be modified and distributed under the terms
# of the BSD license. See the LICENSE file for details.

"""Python interface to the Zstandard (zstd) compression library."""

from __future__ import absolute_import, unicode_literals

import io

from _zstd_cffi import (
    ffi,
    lib,
)


_CSTREAM_IN_SIZE = lib.ZSTD_CStreamInSize()
_CSTREAM_OUT_SIZE = lib.ZSTD_CStreamOutSize()


class _ZstdCompressionWriter(object):
    def __init__(self, cstream, writer):
        self._cstream = cstream
        self._writer = writer

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, exc_tb):
        if not exc_type and not exc_value and not exc_tb:
            out_buffer = ffi.new('ZSTD_outBuffer *')
            out_buffer.dst = ffi.new('char[]', _CSTREAM_OUT_SIZE)
            out_buffer.size = _CSTREAM_OUT_SIZE
            out_buffer.pos = 0

            while True:
                res = lib.ZSTD_endStream(self._cstream, out_buffer)
                if lib.ZSTD_isError(res):
                    raise Exception('error ending compression stream: %s' % lib.ZSTD_getErrorName)

                if out_buffer.pos:
                    self._writer.write(ffi.buffer(out_buffer.dst, out_buffer.pos))
                    out_buffer.pos = 0

                if res == 0:
                    break

        return False

    def write(self, data):
        out_buffer = ffi.new('ZSTD_outBuffer *')
        out_buffer.dst = ffi.new('char[]', _CSTREAM_OUT_SIZE)
        out_buffer.size = _CSTREAM_OUT_SIZE
        out_buffer.pos = 0

        # TODO can we reuse existing memory?
        in_buffer = ffi.new('ZSTD_inBuffer *')
        in_buffer.src = ffi.new('char[]', data)
        in_buffer.size = len(data)
        in_buffer.pos = 0
        while in_buffer.pos < in_buffer.size:
            res = lib.ZSTD_compressStream(self._cstream, out_buffer, in_buffer)
            if lib.ZSTD_isError(res):
                raise Exception('zstd compress error: %s' % lib.ZSTD_getErrorName(res))

            if out_buffer.pos:
                self._writer.write(ffi.buffer(out_buffer.dst, out_buffer.pos))
                out_buffer.pos = 0


class ZstdCompressor(object):
    def __init__(self, level=3, dict_data=None, compression_params=None):
        if dict_data:
            raise Exception('dict_data not yet supported')
        if compression_params:
            raise Exception('compression_params not yet supported')

        self._compression_level = level

    def compress(self, data):
        # Just use the stream API for now.
        output = io.BytesIO()
        with self.write_to(output) as compressor:
            compressor.write(data)
        return output.getvalue()

    def copy_stream(self, ifh, ofh):
        cstream = self._get_cstream()

        in_buffer = ffi.new('ZSTD_inBuffer *')
        out_buffer = ffi.new('ZSTD_outBuffer *')

        out_buffer.dst = ffi.new('char[]', _CSTREAM_OUT_SIZE)
        out_buffer.size = _CSTREAM_OUT_SIZE
        out_buffer.pos = 0

        total_read, total_write = 0, 0

        while True:
            data = ifh.read(_CSTREAM_IN_SIZE)
            if not data:
                break

            total_read += len(data)

            in_buffer.src = ffi.new('char[]', data)
            in_buffer.size = len(data)
            in_buffer.pos = 0

            while in_buffer.pos < in_buffer.size:
                res = lib.ZSTD_compressStream(cstream, out_buffer, in_buffer)
                if lib.ZSTD_isError(res):
                    raise Exception('zstd compress error: %s' %
                                    lib.ZSTD_getErrorName(res))

                if out_buffer.pos:
                    ofh.write(ffi.buffer(out_buffer.dst, out_buffer.pos))
                    total_write = out_buffer.pos
                    out_buffer.pos = 0

        # We've finished reading. Flush the compressor.
        while True:
            res = lib.ZSTD_endStream(cstream, out_buffer)
            if lib.ZSTD_isError(res):
                raise Exception('error ending compression stream: %s' %
                                lib.ZSTD_getErrorName(res))

            if out_buffer.pos:
                ofh.write(ffi.buffer(out_buffer.dst, out_buffer.pos))
                total_write += out_buffer.pos
                out_buffer.pos = 0

            if res == 0:
                break

        return total_read, total_write

    def write_to(self, writer):
        return _ZstdCompressionWriter(self._get_cstream(), writer)

    def _get_cstream(self):
        cstream = lib.ZSTD_createCStream()
        cstream = ffi.gc(cstream, lib.ZSTD_freeCStream)

        res = lib.ZSTD_initCStream(cstream, self._compression_level)
        if lib.ZSTD_isError(res):
            raise Exception('cannot init CStream: %s' %
                            lib.ZSTD_getErrorName(res))

        return cstream

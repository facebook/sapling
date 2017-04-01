# Copyright (c) 2016-present, Gregory Szorc
# All rights reserved.
#
# This software may be modified and distributed under the terms
# of the BSD license. See the LICENSE file for details.

"""Python interface to the Zstandard (zstd) compression library."""

from __future__ import absolute_import, unicode_literals

import os
import sys

from _zstd_cffi import (
    ffi,
    lib,
)

if sys.version_info[0] == 2:
    bytes_type = str
    int_type = long
else:
    bytes_type = bytes
    int_type = int


COMPRESSION_RECOMMENDED_INPUT_SIZE = lib.ZSTD_CStreamInSize()
COMPRESSION_RECOMMENDED_OUTPUT_SIZE = lib.ZSTD_CStreamOutSize()
DECOMPRESSION_RECOMMENDED_INPUT_SIZE = lib.ZSTD_DStreamInSize()
DECOMPRESSION_RECOMMENDED_OUTPUT_SIZE = lib.ZSTD_DStreamOutSize()

new_nonzero = ffi.new_allocator(should_clear_after_alloc=False)


MAX_COMPRESSION_LEVEL = lib.ZSTD_maxCLevel()
MAGIC_NUMBER = lib.ZSTD_MAGICNUMBER
FRAME_HEADER = b'\x28\xb5\x2f\xfd'
ZSTD_VERSION = (lib.ZSTD_VERSION_MAJOR, lib.ZSTD_VERSION_MINOR, lib.ZSTD_VERSION_RELEASE)

WINDOWLOG_MIN = lib.ZSTD_WINDOWLOG_MIN
WINDOWLOG_MAX = lib.ZSTD_WINDOWLOG_MAX
CHAINLOG_MIN = lib.ZSTD_CHAINLOG_MIN
CHAINLOG_MAX = lib.ZSTD_CHAINLOG_MAX
HASHLOG_MIN = lib.ZSTD_HASHLOG_MIN
HASHLOG_MAX = lib.ZSTD_HASHLOG_MAX
HASHLOG3_MAX = lib.ZSTD_HASHLOG3_MAX
SEARCHLOG_MIN = lib.ZSTD_SEARCHLOG_MIN
SEARCHLOG_MAX = lib.ZSTD_SEARCHLOG_MAX
SEARCHLENGTH_MIN = lib.ZSTD_SEARCHLENGTH_MIN
SEARCHLENGTH_MAX = lib.ZSTD_SEARCHLENGTH_MAX
TARGETLENGTH_MIN = lib.ZSTD_TARGETLENGTH_MIN
TARGETLENGTH_MAX = lib.ZSTD_TARGETLENGTH_MAX

STRATEGY_FAST = lib.ZSTD_fast
STRATEGY_DFAST = lib.ZSTD_dfast
STRATEGY_GREEDY = lib.ZSTD_greedy
STRATEGY_LAZY = lib.ZSTD_lazy
STRATEGY_LAZY2 = lib.ZSTD_lazy2
STRATEGY_BTLAZY2 = lib.ZSTD_btlazy2
STRATEGY_BTOPT = lib.ZSTD_btopt

COMPRESSOBJ_FLUSH_FINISH = 0
COMPRESSOBJ_FLUSH_BLOCK = 1


def _cpu_count():
    # os.cpu_count() was introducd in Python 3.4.
    try:
        return os.cpu_count() or 0
    except AttributeError:
        pass

    # Linux.
    try:
        if sys.version_info[0] == 2:
            return os.sysconf(b'SC_NPROCESSORS_ONLN')
        else:
            return os.sysconf(u'SC_NPROCESSORS_ONLN')
    except (AttributeError, ValueError):
        pass

    # TODO implement on other platforms.
    return 0


class ZstdError(Exception):
    pass


class CompressionParameters(object):
    def __init__(self, window_log, chain_log, hash_log, search_log,
                 search_length, target_length, strategy):
        if window_log < WINDOWLOG_MIN or window_log > WINDOWLOG_MAX:
            raise ValueError('invalid window log value')

        if chain_log < CHAINLOG_MIN or chain_log > CHAINLOG_MAX:
            raise ValueError('invalid chain log value')

        if hash_log < HASHLOG_MIN or hash_log > HASHLOG_MAX:
            raise ValueError('invalid hash log value')

        if search_log < SEARCHLOG_MIN or search_log > SEARCHLOG_MAX:
            raise ValueError('invalid search log value')

        if search_length < SEARCHLENGTH_MIN or search_length > SEARCHLENGTH_MAX:
            raise ValueError('invalid search length value')

        if target_length < TARGETLENGTH_MIN or target_length > TARGETLENGTH_MAX:
            raise ValueError('invalid target length value')

        if strategy < STRATEGY_FAST or strategy > STRATEGY_BTOPT:
            raise ValueError('invalid strategy value')

        self.window_log = window_log
        self.chain_log = chain_log
        self.hash_log = hash_log
        self.search_log = search_log
        self.search_length = search_length
        self.target_length = target_length
        self.strategy = strategy

        zresult = lib.ZSTD_checkCParams(self.as_compression_parameters())
        if lib.ZSTD_isError(zresult):
            raise ValueError('invalid compression parameters: %s',
                             ffi.string(lib.ZSTD_getErrorName(zresult)))

    def estimated_compression_context_size(self):
        return lib.ZSTD_estimateCCtxSize(self.as_compression_parameters())

    def as_compression_parameters(self):
        p = ffi.new('ZSTD_compressionParameters *')[0]
        p.windowLog = self.window_log
        p.chainLog = self.chain_log
        p.hashLog = self.hash_log
        p.searchLog = self.search_log
        p.searchLength = self.search_length
        p.targetLength = self.target_length
        p.strategy = self.strategy

        return p

def get_compression_parameters(level, source_size=0, dict_size=0):
    params = lib.ZSTD_getCParams(level, source_size, dict_size)
    return CompressionParameters(window_log=params.windowLog,
                                 chain_log=params.chainLog,
                                 hash_log=params.hashLog,
                                 search_log=params.searchLog,
                                 search_length=params.searchLength,
                                 target_length=params.targetLength,
                                 strategy=params.strategy)


def estimate_compression_context_size(params):
    if not isinstance(params, CompressionParameters):
        raise ValueError('argument must be a CompressionParameters')

    cparams = params.as_compression_parameters()
    return lib.ZSTD_estimateCCtxSize(cparams)


def estimate_decompression_context_size():
    return lib.ZSTD_estimateDCtxSize()


class ZstdCompressionWriter(object):
    def __init__(self, compressor, writer, source_size, write_size):
        self._compressor = compressor
        self._writer = writer
        self._source_size = source_size
        self._write_size = write_size
        self._entered = False
        self._mtcctx = compressor._cctx if compressor._multithreaded else None

    def __enter__(self):
        if self._entered:
            raise ZstdError('cannot __enter__ multiple times')

        if self._mtcctx:
            self._compressor._init_mtcstream(self._source_size)
        else:
            self._compressor._ensure_cstream(self._source_size)
        self._entered = True
        return self

    def __exit__(self, exc_type, exc_value, exc_tb):
        self._entered = False

        if not exc_type and not exc_value and not exc_tb:
            out_buffer = ffi.new('ZSTD_outBuffer *')
            dst_buffer = ffi.new('char[]', self._write_size)
            out_buffer.dst = dst_buffer
            out_buffer.size = self._write_size
            out_buffer.pos = 0

            while True:
                if self._mtcctx:
                    zresult = lib.ZSTDMT_endStream(self._mtcctx, out_buffer)
                else:
                    zresult = lib.ZSTD_endStream(self._compressor._cstream, out_buffer)
                if lib.ZSTD_isError(zresult):
                    raise ZstdError('error ending compression stream: %s' %
                                    ffi.string(lib.ZSTD_getErrorName(zresult)))

                if out_buffer.pos:
                    self._writer.write(ffi.buffer(out_buffer.dst, out_buffer.pos)[:])
                    out_buffer.pos = 0

                if zresult == 0:
                    break

        self._compressor = None

        return False

    def memory_size(self):
        if not self._entered:
            raise ZstdError('cannot determine size of an inactive compressor; '
                            'call when a context manager is active')

        return lib.ZSTD_sizeof_CStream(self._compressor._cstream)

    def write(self, data):
        if not self._entered:
            raise ZstdError('write() must be called from an active context '
                            'manager')

        total_write = 0

        data_buffer = ffi.from_buffer(data)

        in_buffer = ffi.new('ZSTD_inBuffer *')
        in_buffer.src = data_buffer
        in_buffer.size = len(data_buffer)
        in_buffer.pos = 0

        out_buffer = ffi.new('ZSTD_outBuffer *')
        dst_buffer = ffi.new('char[]', self._write_size)
        out_buffer.dst = dst_buffer
        out_buffer.size = self._write_size
        out_buffer.pos = 0

        while in_buffer.pos < in_buffer.size:
            if self._mtcctx:
                zresult = lib.ZSTDMT_compressStream(self._mtcctx, out_buffer,
                                                    in_buffer)
            else:
                zresult = lib.ZSTD_compressStream(self._compressor._cstream, out_buffer,
                                                  in_buffer)
            if lib.ZSTD_isError(zresult):
                raise ZstdError('zstd compress error: %s' %
                                ffi.string(lib.ZSTD_getErrorName(zresult)))

            if out_buffer.pos:
                self._writer.write(ffi.buffer(out_buffer.dst, out_buffer.pos)[:])
                total_write += out_buffer.pos
                out_buffer.pos = 0

        return total_write

    def flush(self):
        if not self._entered:
            raise ZstdError('flush must be called from an active context manager')

        total_write = 0

        out_buffer = ffi.new('ZSTD_outBuffer *')
        dst_buffer = ffi.new('char[]', self._write_size)
        out_buffer.dst = dst_buffer
        out_buffer.size = self._write_size
        out_buffer.pos = 0

        while True:
            if self._mtcctx:
                zresult = lib.ZSTDMT_flushStream(self._mtcctx, out_buffer)
            else:
                zresult = lib.ZSTD_flushStream(self._compressor._cstream, out_buffer)
            if lib.ZSTD_isError(zresult):
                raise ZstdError('zstd compress error: %s' %
                                ffi.string(lib.ZSTD_getErrorName(zresult)))

            if not out_buffer.pos:
                break

            self._writer.write(ffi.buffer(out_buffer.dst, out_buffer.pos)[:])
            total_write += out_buffer.pos
            out_buffer.pos = 0

        return total_write


class ZstdCompressionObj(object):
    def compress(self, data):
        if self._finished:
            raise ZstdError('cannot call compress() after compressor finished')

        data_buffer = ffi.from_buffer(data)
        source = ffi.new('ZSTD_inBuffer *')
        source.src = data_buffer
        source.size = len(data_buffer)
        source.pos = 0

        chunks = []

        while source.pos < len(data):
            if self._mtcctx:
                zresult = lib.ZSTDMT_compressStream(self._mtcctx,
                                                    self._out, source)
            else:
                zresult = lib.ZSTD_compressStream(self._compressor._cstream, self._out,
                                                  source)
            if lib.ZSTD_isError(zresult):
                raise ZstdError('zstd compress error: %s' %
                                ffi.string(lib.ZSTD_getErrorName(zresult)))

            if self._out.pos:
                chunks.append(ffi.buffer(self._out.dst, self._out.pos)[:])
                self._out.pos = 0

        return b''.join(chunks)

    def flush(self, flush_mode=COMPRESSOBJ_FLUSH_FINISH):
        if flush_mode not in (COMPRESSOBJ_FLUSH_FINISH, COMPRESSOBJ_FLUSH_BLOCK):
            raise ValueError('flush mode not recognized')

        if self._finished:
            raise ZstdError('compressor object already finished')

        assert self._out.pos == 0

        if flush_mode == COMPRESSOBJ_FLUSH_BLOCK:
            if self._mtcctx:
                zresult = lib.ZSTDMT_flushStream(self._mtcctx, self._out)
            else:
                zresult = lib.ZSTD_flushStream(self._compressor._cstream, self._out)
            if lib.ZSTD_isError(zresult):
                raise ZstdError('zstd compress error: %s' %
                                ffi.string(lib.ZSTD_getErrorName(zresult)))

            # Output buffer is guaranteed to hold full block.
            assert zresult == 0

            if self._out.pos:
                result = ffi.buffer(self._out.dst, self._out.pos)[:]
                self._out.pos = 0
                return result
            else:
                return b''

        assert flush_mode == COMPRESSOBJ_FLUSH_FINISH
        self._finished = True

        chunks = []

        while True:
            if self._mtcctx:
                zresult = lib.ZSTDMT_endStream(self._mtcctx, self._out)
            else:
                zresult = lib.ZSTD_endStream(self._compressor._cstream, self._out)
            if lib.ZSTD_isError(zresult):
                raise ZstdError('error ending compression stream: %s' %
                                ffi.string(lib.ZSTD_getErroName(zresult)))

            if self._out.pos:
                chunks.append(ffi.buffer(self._out.dst, self._out.pos)[:])
                self._out.pos = 0

            if not zresult:
                break

        return b''.join(chunks)


class ZstdCompressor(object):
    def __init__(self, level=3, dict_data=None, compression_params=None,
                 write_checksum=False, write_content_size=False,
                 write_dict_id=True, threads=0):
        if level < 1:
            raise ValueError('level must be greater than 0')
        elif level > lib.ZSTD_maxCLevel():
            raise ValueError('level must be less than %d' % lib.ZSTD_maxCLevel())

        if threads < 0:
            threads = _cpu_count()

        self._compression_level = level
        self._dict_data = dict_data
        self._cparams = compression_params
        self._fparams = ffi.new('ZSTD_frameParameters *')[0]
        self._fparams.checksumFlag = write_checksum
        self._fparams.contentSizeFlag = write_content_size
        self._fparams.noDictIDFlag = not write_dict_id

        if threads:
            cctx = lib.ZSTDMT_createCCtx(threads)
            if cctx == ffi.NULL:
                raise MemoryError()

            self._cctx = ffi.gc(cctx, lib.ZSTDMT_freeCCtx)
            self._multithreaded = True
        else:
            cctx = lib.ZSTD_createCCtx()
            if cctx == ffi.NULL:
                raise MemoryError()

            self._cctx = ffi.gc(cctx, lib.ZSTD_freeCCtx)
            self._multithreaded = False

        self._cstream = None

    def compress(self, data, allow_empty=False):
        if len(data) == 0 and self._fparams.contentSizeFlag and not allow_empty:
            raise ValueError('cannot write empty inputs when writing content sizes')

        if self._multithreaded and self._dict_data:
            raise ZstdError('compress() cannot be used with both dictionaries and multi-threaded compression')

        if self._multithreaded and self._cparams:
            raise ZstdError('compress() cannot be used with both compression parameters and multi-threaded compression')

        # TODO use a CDict for performance.
        dict_data = ffi.NULL
        dict_size = 0

        if self._dict_data:
            dict_data = self._dict_data.as_bytes()
            dict_size = len(self._dict_data)

        params = ffi.new('ZSTD_parameters *')[0]
        if self._cparams:
            params.cParams = self._cparams.as_compression_parameters()
        else:
            params.cParams = lib.ZSTD_getCParams(self._compression_level, len(data),
                                                 dict_size)
        params.fParams = self._fparams

        dest_size = lib.ZSTD_compressBound(len(data))
        out = new_nonzero('char[]', dest_size)

        if self._multithreaded:
            zresult = lib.ZSTDMT_compressCCtx(self._cctx,
                                              ffi.addressof(out), dest_size,
                                              data, len(data),
                                              self._compression_level)
        else:
            zresult = lib.ZSTD_compress_advanced(self._cctx,
                                                 ffi.addressof(out), dest_size,
                                                 data, len(data),
                                                 dict_data, dict_size,
                                                 params)

        if lib.ZSTD_isError(zresult):
            raise ZstdError('cannot compress: %s' %
                            ffi.string(lib.ZSTD_getErrorName(zresult)))

        return ffi.buffer(out, zresult)[:]

    def compressobj(self, size=0):
        if self._multithreaded:
            self._init_mtcstream(size)
        else:
            self._ensure_cstream(size)

        cobj = ZstdCompressionObj()
        cobj._out = ffi.new('ZSTD_outBuffer *')
        cobj._dst_buffer = ffi.new('char[]', COMPRESSION_RECOMMENDED_OUTPUT_SIZE)
        cobj._out.dst = cobj._dst_buffer
        cobj._out.size = COMPRESSION_RECOMMENDED_OUTPUT_SIZE
        cobj._out.pos = 0
        cobj._compressor = self
        cobj._finished = False

        if self._multithreaded:
            cobj._mtcctx = self._cctx
        else:
            cobj._mtcctx = None

        return cobj

    def copy_stream(self, ifh, ofh, size=0,
                    read_size=COMPRESSION_RECOMMENDED_INPUT_SIZE,
                    write_size=COMPRESSION_RECOMMENDED_OUTPUT_SIZE):

        if not hasattr(ifh, 'read'):
            raise ValueError('first argument must have a read() method')
        if not hasattr(ofh, 'write'):
            raise ValueError('second argument must have a write() method')

        mt = self._multithreaded
        if mt:
            self._init_mtcstream(size)
        else:
            self._ensure_cstream(size)

        in_buffer = ffi.new('ZSTD_inBuffer *')
        out_buffer = ffi.new('ZSTD_outBuffer *')

        dst_buffer = ffi.new('char[]', write_size)
        out_buffer.dst = dst_buffer
        out_buffer.size = write_size
        out_buffer.pos = 0

        total_read, total_write = 0, 0

        while True:
            data = ifh.read(read_size)
            if not data:
                break

            data_buffer = ffi.from_buffer(data)
            total_read += len(data_buffer)
            in_buffer.src = data_buffer
            in_buffer.size = len(data_buffer)
            in_buffer.pos = 0

            while in_buffer.pos < in_buffer.size:
                if mt:
                    zresult = lib.ZSTDMT_compressStream(self._cctx, out_buffer, in_buffer)
                else:
                    zresult = lib.ZSTD_compressStream(self._cstream,
                                                      out_buffer, in_buffer)
                if lib.ZSTD_isError(zresult):
                    raise ZstdError('zstd compress error: %s' %
                                    ffi.string(lib.ZSTD_getErrorName(zresult)))

                if out_buffer.pos:
                    ofh.write(ffi.buffer(out_buffer.dst, out_buffer.pos))
                    total_write += out_buffer.pos
                    out_buffer.pos = 0

        # We've finished reading. Flush the compressor.
        while True:
            if mt:
                zresult = lib.ZSTDMT_endStream(self._cctx, out_buffer)
            else:
                zresult = lib.ZSTD_endStream(self._cstream, out_buffer)
            if lib.ZSTD_isError(zresult):
                raise ZstdError('error ending compression stream: %s' %
                                ffi.string(lib.ZSTD_getErrorName(zresult)))

            if out_buffer.pos:
                ofh.write(ffi.buffer(out_buffer.dst, out_buffer.pos))
                total_write += out_buffer.pos
                out_buffer.pos = 0

            if zresult == 0:
                break

        return total_read, total_write

    def write_to(self, writer, size=0,
                 write_size=COMPRESSION_RECOMMENDED_OUTPUT_SIZE):

        if not hasattr(writer, 'write'):
            raise ValueError('must pass an object with a write() method')

        return ZstdCompressionWriter(self, writer, size, write_size)

    def read_from(self, reader, size=0,
                  read_size=COMPRESSION_RECOMMENDED_INPUT_SIZE,
                  write_size=COMPRESSION_RECOMMENDED_OUTPUT_SIZE):
        if hasattr(reader, 'read'):
            have_read = True
        elif hasattr(reader, '__getitem__'):
            have_read = False
            buffer_offset = 0
            size = len(reader)
        else:
            raise ValueError('must pass an object with a read() method or '
                             'conforms to buffer protocol')

        if self._multithreaded:
            self._init_mtcstream(size)
        else:
            self._ensure_cstream(size)

        in_buffer = ffi.new('ZSTD_inBuffer *')
        out_buffer = ffi.new('ZSTD_outBuffer *')

        in_buffer.src = ffi.NULL
        in_buffer.size = 0
        in_buffer.pos = 0

        dst_buffer = ffi.new('char[]', write_size)
        out_buffer.dst = dst_buffer
        out_buffer.size = write_size
        out_buffer.pos = 0

        while True:
            # We should never have output data sitting around after a previous
            # iteration.
            assert out_buffer.pos == 0

            # Collect input data.
            if have_read:
                read_result = reader.read(read_size)
            else:
                remaining = len(reader) - buffer_offset
                slice_size = min(remaining, read_size)
                read_result = reader[buffer_offset:buffer_offset + slice_size]
                buffer_offset += slice_size

            # No new input data. Break out of the read loop.
            if not read_result:
                break

            # Feed all read data into the compressor and emit output until
            # exhausted.
            read_buffer = ffi.from_buffer(read_result)
            in_buffer.src = read_buffer
            in_buffer.size = len(read_buffer)
            in_buffer.pos = 0

            while in_buffer.pos < in_buffer.size:
                if self._multithreaded:
                    zresult = lib.ZSTDMT_compressStream(self._cctx, out_buffer, in_buffer)
                else:
                    zresult = lib.ZSTD_compressStream(self._cstream, out_buffer, in_buffer)
                if lib.ZSTD_isError(zresult):
                    raise ZstdError('zstd compress error: %s' %
                                    ffi.string(lib.ZSTD_getErrorName(zresult)))

                if out_buffer.pos:
                    data = ffi.buffer(out_buffer.dst, out_buffer.pos)[:]
                    out_buffer.pos = 0
                    yield data

            assert out_buffer.pos == 0

            # And repeat the loop to collect more data.
            continue

        # If we get here, input is exhausted. End the stream and emit what
        # remains.
        while True:
            assert out_buffer.pos == 0
            if self._multithreaded:
                zresult = lib.ZSTDMT_endStream(self._cctx, out_buffer)
            else:
                zresult = lib.ZSTD_endStream(self._cstream, out_buffer)
            if lib.ZSTD_isError(zresult):
                raise ZstdError('error ending compression stream: %s' %
                                ffi.string(lib.ZSTD_getErrorName(zresult)))

            if out_buffer.pos:
                data = ffi.buffer(out_buffer.dst, out_buffer.pos)[:]
                out_buffer.pos = 0
                yield data

            if zresult == 0:
                break

    def _ensure_cstream(self, size):
        if self._cstream:
            zresult = lib.ZSTD_resetCStream(self._cstream, size)
            if lib.ZSTD_isError(zresult):
                raise ZstdError('could not reset CStream: %s' %
                                ffi.string(lib.ZSTD_getErrorName(zresult)))

            return

        cstream = lib.ZSTD_createCStream()
        if cstream == ffi.NULL:
            raise MemoryError()

        cstream = ffi.gc(cstream, lib.ZSTD_freeCStream)

        dict_data = ffi.NULL
        dict_size = 0
        if self._dict_data:
            dict_data = self._dict_data.as_bytes()
            dict_size = len(self._dict_data)

        zparams = ffi.new('ZSTD_parameters *')[0]
        if self._cparams:
            zparams.cParams = self._cparams.as_compression_parameters()
        else:
            zparams.cParams = lib.ZSTD_getCParams(self._compression_level,
                                                  size, dict_size)
        zparams.fParams = self._fparams

        zresult = lib.ZSTD_initCStream_advanced(cstream, dict_data, dict_size,
                                                zparams, size)
        if lib.ZSTD_isError(zresult):
            raise Exception('cannot init CStream: %s' %
                            ffi.string(lib.ZSTD_getErrorName(zresult)))

        self._cstream = cstream

    def _init_mtcstream(self, size):
        assert self._multithreaded

        dict_data = ffi.NULL
        dict_size = 0
        if self._dict_data:
            dict_data = self._dict_data.as_bytes()
            dict_size = len(self._dict_data)

        zparams = ffi.new('ZSTD_parameters *')[0]
        if self._cparams:
            zparams.cParams = self._cparams.as_compression_parameters()
        else:
            zparams.cParams = lib.ZSTD_getCParams(self._compression_level,
                                                  size, dict_size)

        zparams.fParams = self._fparams

        zresult = lib.ZSTDMT_initCStream_advanced(self._cctx, dict_data, dict_size,
                                                  zparams, size)

        if lib.ZSTD_isError(zresult):
            raise ZstdError('cannot init CStream: %s' %
                            ffi.string(lib.ZSTD_getErrorName(zresult)))


class FrameParameters(object):
    def __init__(self, fparams):
        self.content_size = fparams.frameContentSize
        self.window_size = fparams.windowSize
        self.dict_id = fparams.dictID
        self.has_checksum = bool(fparams.checksumFlag)


def get_frame_parameters(data):
    if not isinstance(data, bytes_type):
        raise TypeError('argument must be bytes')

    params = ffi.new('ZSTD_frameParams *')

    zresult = lib.ZSTD_getFrameParams(params, data, len(data))
    if lib.ZSTD_isError(zresult):
        raise ZstdError('cannot get frame parameters: %s' %
                        ffi.string(lib.ZSTD_getErrorName(zresult)))

    if zresult:
        raise ZstdError('not enough data for frame parameters; need %d bytes' %
                        zresult)

    return FrameParameters(params[0])


class ZstdCompressionDict(object):
    def __init__(self, data, k=0, d=0):
        assert isinstance(data, bytes_type)
        self._data = data
        self.k = k
        self.d = d

    def __len__(self):
        return len(self._data)

    def dict_id(self):
        return int_type(lib.ZDICT_getDictID(self._data, len(self._data)))

    def as_bytes(self):
        return self._data


def train_dictionary(dict_size, samples, selectivity=0, level=0,
                     notifications=0, dict_id=0):
    if not isinstance(samples, list):
        raise TypeError('samples must be a list')

    total_size = sum(map(len, samples))

    samples_buffer = new_nonzero('char[]', total_size)
    sample_sizes = new_nonzero('size_t[]', len(samples))

    offset = 0
    for i, sample in enumerate(samples):
        if not isinstance(sample, bytes_type):
            raise ValueError('samples must be bytes')

        l = len(sample)
        ffi.memmove(samples_buffer + offset, sample, l)
        offset += l
        sample_sizes[i] = l

    dict_data = new_nonzero('char[]', dict_size)

    dparams = ffi.new('ZDICT_params_t *')[0]
    dparams.selectivityLevel = selectivity
    dparams.compressionLevel = level
    dparams.notificationLevel = notifications
    dparams.dictID = dict_id

    zresult = lib.ZDICT_trainFromBuffer_advanced(
        ffi.addressof(dict_data), dict_size,
        ffi.addressof(samples_buffer),
        ffi.addressof(sample_sizes, 0), len(samples),
        dparams)

    if lib.ZDICT_isError(zresult):
        raise ZstdError('Cannot train dict: %s' %
                        ffi.string(lib.ZDICT_getErrorName(zresult)))

    return ZstdCompressionDict(ffi.buffer(dict_data, zresult)[:])


def train_cover_dictionary(dict_size, samples, k=0, d=0,
                           notifications=0, dict_id=0, level=0, optimize=False,
                           steps=0, threads=0):
    if not isinstance(samples, list):
        raise TypeError('samples must be a list')

    if threads < 0:
        threads = _cpu_count()

    total_size = sum(map(len, samples))

    samples_buffer = new_nonzero('char[]', total_size)
    sample_sizes = new_nonzero('size_t[]', len(samples))

    offset = 0
    for i, sample in enumerate(samples):
        if not isinstance(sample, bytes_type):
            raise ValueError('samples must be bytes')

        l = len(sample)
        ffi.memmove(samples_buffer + offset, sample, l)
        offset += l
        sample_sizes[i] = l

    dict_data = new_nonzero('char[]', dict_size)

    dparams = ffi.new('COVER_params_t *')[0]
    dparams.k = k
    dparams.d = d
    dparams.steps = steps
    dparams.nbThreads = threads
    dparams.notificationLevel = notifications
    dparams.dictID = dict_id
    dparams.compressionLevel = level

    if optimize:
        zresult = lib.COVER_optimizeTrainFromBuffer(
            ffi.addressof(dict_data), dict_size,
            ffi.addressof(samples_buffer),
            ffi.addressof(sample_sizes, 0), len(samples),
            ffi.addressof(dparams))
    else:
        zresult = lib.COVER_trainFromBuffer(
            ffi.addressof(dict_data), dict_size,
            ffi.addressof(samples_buffer),
            ffi.addressof(sample_sizes, 0), len(samples),
            dparams)

    if lib.ZDICT_isError(zresult):
        raise ZstdError('cannot train dict: %s' %
                        ffi.string(lib.ZDICT_getErrorName(zresult)))

    return ZstdCompressionDict(ffi.buffer(dict_data, zresult)[:],
                               k=dparams.k, d=dparams.d)


class ZstdDecompressionObj(object):
    def __init__(self, decompressor):
        self._decompressor = decompressor
        self._finished = False

    def decompress(self, data):
        if self._finished:
            raise ZstdError('cannot use a decompressobj multiple times')

        assert(self._decompressor._dstream)

        in_buffer = ffi.new('ZSTD_inBuffer *')
        out_buffer = ffi.new('ZSTD_outBuffer *')

        data_buffer = ffi.from_buffer(data)
        in_buffer.src = data_buffer
        in_buffer.size = len(data_buffer)
        in_buffer.pos = 0

        dst_buffer = ffi.new('char[]', DECOMPRESSION_RECOMMENDED_OUTPUT_SIZE)
        out_buffer.dst = dst_buffer
        out_buffer.size = len(dst_buffer)
        out_buffer.pos = 0

        chunks = []

        while in_buffer.pos < in_buffer.size:
            zresult = lib.ZSTD_decompressStream(self._decompressor._dstream,
                                                out_buffer, in_buffer)
            if lib.ZSTD_isError(zresult):
                raise ZstdError('zstd decompressor error: %s' %
                                ffi.string(lib.ZSTD_getErrorName(zresult)))

            if zresult == 0:
                self._finished = True
                self._decompressor = None

            if out_buffer.pos:
                chunks.append(ffi.buffer(out_buffer.dst, out_buffer.pos)[:])
                out_buffer.pos = 0

        return b''.join(chunks)


class ZstdDecompressionWriter(object):
    def __init__(self, decompressor, writer, write_size):
        self._decompressor = decompressor
        self._writer = writer
        self._write_size = write_size
        self._entered = False

    def __enter__(self):
        if self._entered:
            raise ZstdError('cannot __enter__ multiple times')

        self._decompressor._ensure_dstream()
        self._entered = True

        return self

    def __exit__(self, exc_type, exc_value, exc_tb):
        self._entered = False

    def memory_size(self):
        if not self._decompressor._dstream:
            raise ZstdError('cannot determine size of inactive decompressor '
                            'call when context manager is active')

        return lib.ZSTD_sizeof_DStream(self._decompressor._dstream)

    def write(self, data):
        if not self._entered:
            raise ZstdError('write must be called from an active context manager')

        total_write = 0

        in_buffer = ffi.new('ZSTD_inBuffer *')
        out_buffer = ffi.new('ZSTD_outBuffer *')

        data_buffer = ffi.from_buffer(data)
        in_buffer.src = data_buffer
        in_buffer.size = len(data_buffer)
        in_buffer.pos = 0

        dst_buffer = ffi.new('char[]', self._write_size)
        out_buffer.dst = dst_buffer
        out_buffer.size = len(dst_buffer)
        out_buffer.pos = 0

        dstream = self._decompressor._dstream

        while in_buffer.pos < in_buffer.size:
            zresult = lib.ZSTD_decompressStream(dstream, out_buffer, in_buffer)
            if lib.ZSTD_isError(zresult):
                raise ZstdError('zstd decompress error: %s' %
                                ffi.string(lib.ZSTD_getErrorName(zresult)))

            if out_buffer.pos:
                self._writer.write(ffi.buffer(out_buffer.dst, out_buffer.pos)[:])
                total_write += out_buffer.pos
                out_buffer.pos = 0

        return total_write


class ZstdDecompressor(object):
    def __init__(self, dict_data=None):
        self._dict_data = dict_data

        dctx = lib.ZSTD_createDCtx()
        if dctx == ffi.NULL:
            raise MemoryError()

        self._refdctx = ffi.gc(dctx, lib.ZSTD_freeDCtx)
        self._dstream = None

    @property
    def _ddict(self):
        if self._dict_data:
            dict_data = self._dict_data.as_bytes()
            dict_size = len(self._dict_data)

            ddict = lib.ZSTD_createDDict(dict_data, dict_size)
            if ddict == ffi.NULL:
                raise ZstdError('could not create decompression dict')
        else:
            ddict = None

        self.__dict__['_ddict'] = ddict
        return ddict

    def decompress(self, data, max_output_size=0):
        data_buffer = ffi.from_buffer(data)

        orig_dctx = new_nonzero('char[]', lib.ZSTD_sizeof_DCtx(self._refdctx))
        dctx = ffi.cast('ZSTD_DCtx *', orig_dctx)
        lib.ZSTD_copyDCtx(dctx, self._refdctx)

        ddict = self._ddict

        output_size = lib.ZSTD_getDecompressedSize(data_buffer, len(data_buffer))
        if output_size:
            result_buffer = ffi.new('char[]', output_size)
            result_size = output_size
        else:
            if not max_output_size:
                raise ZstdError('input data invalid or missing content size '
                                'in frame header')

            result_buffer = ffi.new('char[]', max_output_size)
            result_size = max_output_size

        if ddict:
            zresult = lib.ZSTD_decompress_usingDDict(dctx,
                                                     result_buffer, result_size,
                                                     data_buffer, len(data_buffer),
                                                     ddict)
        else:
            zresult = lib.ZSTD_decompressDCtx(dctx,
                                              result_buffer, result_size,
                                              data_buffer, len(data_buffer))
        if lib.ZSTD_isError(zresult):
            raise ZstdError('decompression error: %s' %
                            ffi.string(lib.ZSTD_getErrorName(zresult)))
        elif output_size and zresult != output_size:
            raise ZstdError('decompression error: decompressed %d bytes; expected %d' %
                            (zresult, output_size))

        return ffi.buffer(result_buffer, zresult)[:]

    def decompressobj(self):
        self._ensure_dstream()
        return ZstdDecompressionObj(self)

    def read_from(self, reader, read_size=DECOMPRESSION_RECOMMENDED_INPUT_SIZE,
                  write_size=DECOMPRESSION_RECOMMENDED_OUTPUT_SIZE,
                  skip_bytes=0):
        if skip_bytes >= read_size:
            raise ValueError('skip_bytes must be smaller than read_size')

        if hasattr(reader, 'read'):
            have_read = True
        elif hasattr(reader, '__getitem__'):
            have_read = False
            buffer_offset = 0
            size = len(reader)
        else:
            raise ValueError('must pass an object with a read() method or '
                             'conforms to buffer protocol')

        if skip_bytes:
            if have_read:
                reader.read(skip_bytes)
            else:
                if skip_bytes > size:
                    raise ValueError('skip_bytes larger than first input chunk')

                buffer_offset = skip_bytes

        self._ensure_dstream()

        in_buffer = ffi.new('ZSTD_inBuffer *')
        out_buffer = ffi.new('ZSTD_outBuffer *')

        dst_buffer = ffi.new('char[]', write_size)
        out_buffer.dst = dst_buffer
        out_buffer.size = len(dst_buffer)
        out_buffer.pos = 0

        while True:
            assert out_buffer.pos == 0

            if have_read:
                read_result = reader.read(read_size)
            else:
                remaining = size - buffer_offset
                slice_size = min(remaining, read_size)
                read_result = reader[buffer_offset:buffer_offset + slice_size]
                buffer_offset += slice_size

            # No new input. Break out of read loop.
            if not read_result:
                break

            # Feed all read data into decompressor and emit output until
            # exhausted.
            read_buffer = ffi.from_buffer(read_result)
            in_buffer.src = read_buffer
            in_buffer.size = len(read_buffer)
            in_buffer.pos = 0

            while in_buffer.pos < in_buffer.size:
                assert out_buffer.pos == 0

                zresult = lib.ZSTD_decompressStream(self._dstream, out_buffer, in_buffer)
                if lib.ZSTD_isError(zresult):
                    raise ZstdError('zstd decompress error: %s' %
                                    ffi.string(lib.ZSTD_getErrorName(zresult)))

                if out_buffer.pos:
                    data = ffi.buffer(out_buffer.dst, out_buffer.pos)[:]
                    out_buffer.pos = 0
                    yield data

                if zresult == 0:
                    return

            # Repeat loop to collect more input data.
            continue

        # If we get here, input is exhausted.

    def write_to(self, writer, write_size=DECOMPRESSION_RECOMMENDED_OUTPUT_SIZE):
        if not hasattr(writer, 'write'):
            raise ValueError('must pass an object with a write() method')

        return ZstdDecompressionWriter(self, writer, write_size)

    def copy_stream(self, ifh, ofh,
                    read_size=DECOMPRESSION_RECOMMENDED_INPUT_SIZE,
                    write_size=DECOMPRESSION_RECOMMENDED_OUTPUT_SIZE):
        if not hasattr(ifh, 'read'):
            raise ValueError('first argument must have a read() method')
        if not hasattr(ofh, 'write'):
            raise ValueError('second argument must have a write() method')

        self._ensure_dstream()

        in_buffer = ffi.new('ZSTD_inBuffer *')
        out_buffer = ffi.new('ZSTD_outBuffer *')

        dst_buffer = ffi.new('char[]', write_size)
        out_buffer.dst = dst_buffer
        out_buffer.size = write_size
        out_buffer.pos = 0

        total_read, total_write = 0, 0

        # Read all available input.
        while True:
            data = ifh.read(read_size)
            if not data:
                break

            data_buffer = ffi.from_buffer(data)
            total_read += len(data_buffer)
            in_buffer.src = data_buffer
            in_buffer.size = len(data_buffer)
            in_buffer.pos = 0

            # Flush all read data to output.
            while in_buffer.pos < in_buffer.size:
                zresult = lib.ZSTD_decompressStream(self._dstream, out_buffer, in_buffer)
                if lib.ZSTD_isError(zresult):
                    raise ZstdError('zstd decompressor error: %s' %
                                    ffi.string(lib.ZSTD_getErrorName(zresult)))

                if out_buffer.pos:
                    ofh.write(ffi.buffer(out_buffer.dst, out_buffer.pos))
                    total_write += out_buffer.pos
                    out_buffer.pos = 0

            # Continue loop to keep reading.

        return total_read, total_write

    def decompress_content_dict_chain(self, frames):
        if not isinstance(frames, list):
            raise TypeError('argument must be a list')

        if not frames:
            raise ValueError('empty input chain')

        # First chunk should not be using a dictionary. We handle it specially.
        chunk = frames[0]
        if not isinstance(chunk, bytes_type):
            raise ValueError('chunk 0 must be bytes')

        # All chunks should be zstd frames and should have content size set.
        chunk_buffer = ffi.from_buffer(chunk)
        params = ffi.new('ZSTD_frameParams *')
        zresult = lib.ZSTD_getFrameParams(params, chunk_buffer, len(chunk_buffer))
        if lib.ZSTD_isError(zresult):
            raise ValueError('chunk 0 is not a valid zstd frame')
        elif zresult:
            raise ValueError('chunk 0 is too small to contain a zstd frame')

        if not params.frameContentSize:
            raise ValueError('chunk 0 missing content size in frame')

        dctx = lib.ZSTD_createDCtx()
        if dctx == ffi.NULL:
            raise MemoryError()

        dctx = ffi.gc(dctx, lib.ZSTD_freeDCtx)

        last_buffer = ffi.new('char[]', params.frameContentSize)

        zresult = lib.ZSTD_decompressDCtx(dctx, last_buffer, len(last_buffer),
                                          chunk_buffer, len(chunk_buffer))
        if lib.ZSTD_isError(zresult):
            raise ZstdError('could not decompress chunk 0: %s' %
                            ffi.string(lib.ZSTD_getErrorName(zresult)))

        # Special case of chain length of 1
        if len(frames) == 1:
            return ffi.buffer(last_buffer, len(last_buffer))[:]

        i = 1
        while i < len(frames):
            chunk = frames[i]
            if not isinstance(chunk, bytes_type):
                raise ValueError('chunk %d must be bytes' % i)

            chunk_buffer = ffi.from_buffer(chunk)
            zresult = lib.ZSTD_getFrameParams(params, chunk_buffer, len(chunk_buffer))
            if lib.ZSTD_isError(zresult):
                raise ValueError('chunk %d is not a valid zstd frame' % i)
            elif zresult:
                raise ValueError('chunk %d is too small to contain a zstd frame' % i)

            if not params.frameContentSize:
                raise ValueError('chunk %d missing content size in frame' % i)

            dest_buffer = ffi.new('char[]', params.frameContentSize)

            zresult = lib.ZSTD_decompress_usingDict(dctx, dest_buffer, len(dest_buffer),
                                                    chunk_buffer, len(chunk_buffer),
                                                    last_buffer, len(last_buffer))
            if lib.ZSTD_isError(zresult):
                raise ZstdError('could not decompress chunk %d' % i)

            last_buffer = dest_buffer
            i += 1

        return ffi.buffer(last_buffer, len(last_buffer))[:]

    def _ensure_dstream(self):
        if self._dstream:
            zresult = lib.ZSTD_resetDStream(self._dstream)
            if lib.ZSTD_isError(zresult):
                raise ZstdError('could not reset DStream: %s' %
                                ffi.string(lib.ZSTD_getErrorName(zresult)))

            return

        self._dstream = lib.ZSTD_createDStream()
        if self._dstream == ffi.NULL:
            raise MemoryError()

        self._dstream = ffi.gc(self._dstream, lib.ZSTD_freeDStream)

        if self._dict_data:
            zresult = lib.ZSTD_initDStream_usingDict(self._dstream,
                                                     self._dict_data.as_bytes(),
                                                     len(self._dict_data))
        else:
            zresult = lib.ZSTD_initDStream(self._dstream)

        if lib.ZSTD_isError(zresult):
            self._dstream = None
            raise ZstdError('could not initialize DStream: %s' %
                            ffi.string(lib.ZSTD_getErrorName(zresult)))

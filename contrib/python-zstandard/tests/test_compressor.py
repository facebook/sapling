import hashlib
import io
import struct
import sys

try:
    import unittest2 as unittest
except ImportError:
    import unittest

import zstd

from .common import (
    make_cffi,
    OpCountingBytesIO,
)


if sys.version_info[0] >= 3:
    next = lambda it: it.__next__()
else:
    next = lambda it: it.next()


def multithreaded_chunk_size(level, source_size=0):
    params = zstd.get_compression_parameters(level, source_size)

    return 1 << (params.window_log + 2)


@make_cffi
class TestCompressor(unittest.TestCase):
    def test_level_bounds(self):
        with self.assertRaises(ValueError):
            zstd.ZstdCompressor(level=0)

        with self.assertRaises(ValueError):
            zstd.ZstdCompressor(level=23)


@make_cffi
class TestCompressor_compress(unittest.TestCase):
    def test_multithreaded_unsupported(self):
        samples = []
        for i in range(128):
            samples.append(b'foo' * 64)
            samples.append(b'bar' * 64)

        d = zstd.train_dictionary(8192, samples)

        cctx = zstd.ZstdCompressor(dict_data=d, threads=2)

        with self.assertRaisesRegexp(zstd.ZstdError, 'compress\(\) cannot be used with both dictionaries and multi-threaded compression'):
            cctx.compress(b'foo')

        params = zstd.get_compression_parameters(3)
        cctx = zstd.ZstdCompressor(compression_params=params, threads=2)
        with self.assertRaisesRegexp(zstd.ZstdError, 'compress\(\) cannot be used with both compression parameters and multi-threaded compression'):
            cctx.compress(b'foo')

    def test_compress_empty(self):
        cctx = zstd.ZstdCompressor(level=1)
        result = cctx.compress(b'')
        self.assertEqual(result, b'\x28\xb5\x2f\xfd\x00\x48\x01\x00\x00')
        params = zstd.get_frame_parameters(result)
        self.assertEqual(params.content_size, 0)
        self.assertEqual(params.window_size, 524288)
        self.assertEqual(params.dict_id, 0)
        self.assertFalse(params.has_checksum, 0)

        # TODO should be temporary until https://github.com/facebook/zstd/issues/506
        # is fixed.
        cctx = zstd.ZstdCompressor(write_content_size=True)
        with self.assertRaises(ValueError):
            cctx.compress(b'')

        cctx.compress(b'', allow_empty=True)

    def test_compress_large(self):
        chunks = []
        for i in range(255):
            chunks.append(struct.Struct('>B').pack(i) * 16384)

        cctx = zstd.ZstdCompressor(level=3)
        result = cctx.compress(b''.join(chunks))
        self.assertEqual(len(result), 999)
        self.assertEqual(result[0:4], b'\x28\xb5\x2f\xfd')

        # This matches the test for read_from() below.
        cctx = zstd.ZstdCompressor(level=1)
        result = cctx.compress(b'f' * zstd.COMPRESSION_RECOMMENDED_INPUT_SIZE + b'o')
        self.assertEqual(result, b'\x28\xb5\x2f\xfd\x00\x40\x54\x00\x00'
                                 b'\x10\x66\x66\x01\x00\xfb\xff\x39\xc0'
                                 b'\x02\x09\x00\x00\x6f')

    def test_write_checksum(self):
        cctx = zstd.ZstdCompressor(level=1)
        no_checksum = cctx.compress(b'foobar')
        cctx = zstd.ZstdCompressor(level=1, write_checksum=True)
        with_checksum = cctx.compress(b'foobar')

        self.assertEqual(len(with_checksum), len(no_checksum) + 4)

        no_params = zstd.get_frame_parameters(no_checksum)
        with_params = zstd.get_frame_parameters(with_checksum)

        self.assertFalse(no_params.has_checksum)
        self.assertTrue(with_params.has_checksum)

    def test_write_content_size(self):
        cctx = zstd.ZstdCompressor(level=1)
        no_size = cctx.compress(b'foobar' * 256)
        cctx = zstd.ZstdCompressor(level=1, write_content_size=True)
        with_size = cctx.compress(b'foobar' * 256)

        self.assertEqual(len(with_size), len(no_size) + 1)

        no_params = zstd.get_frame_parameters(no_size)
        with_params = zstd.get_frame_parameters(with_size)
        self.assertEqual(no_params.content_size, 0)
        self.assertEqual(with_params.content_size, 1536)

    def test_no_dict_id(self):
        samples = []
        for i in range(128):
            samples.append(b'foo' * 64)
            samples.append(b'bar' * 64)
            samples.append(b'foobar' * 64)

        d = zstd.train_dictionary(1024, samples)

        cctx = zstd.ZstdCompressor(level=1, dict_data=d)
        with_dict_id = cctx.compress(b'foobarfoobar')

        cctx = zstd.ZstdCompressor(level=1, dict_data=d, write_dict_id=False)
        no_dict_id = cctx.compress(b'foobarfoobar')

        self.assertEqual(len(with_dict_id), len(no_dict_id) + 4)

        no_params = zstd.get_frame_parameters(no_dict_id)
        with_params = zstd.get_frame_parameters(with_dict_id)
        self.assertEqual(no_params.dict_id, 0)
        self.assertEqual(with_params.dict_id, 1584102229)

    def test_compress_dict_multiple(self):
        samples = []
        for i in range(128):
            samples.append(b'foo' * 64)
            samples.append(b'bar' * 64)
            samples.append(b'foobar' * 64)

        d = zstd.train_dictionary(8192, samples)

        cctx = zstd.ZstdCompressor(level=1, dict_data=d)

        for i in range(32):
            cctx.compress(b'foo bar foobar foo bar foobar')

    def test_multithreaded(self):
        chunk_size = multithreaded_chunk_size(1)
        source = b''.join([b'x' * chunk_size, b'y' * chunk_size])

        cctx = zstd.ZstdCompressor(level=1, threads=2)
        compressed = cctx.compress(source)

        params = zstd.get_frame_parameters(compressed)
        self.assertEqual(params.content_size, chunk_size * 2)
        self.assertEqual(params.dict_id, 0)
        self.assertFalse(params.has_checksum)

        dctx = zstd.ZstdDecompressor()
        self.assertEqual(dctx.decompress(compressed), source)


@make_cffi
class TestCompressor_compressobj(unittest.TestCase):
    def test_compressobj_empty(self):
        cctx = zstd.ZstdCompressor(level=1)
        cobj = cctx.compressobj()
        self.assertEqual(cobj.compress(b''), b'')
        self.assertEqual(cobj.flush(),
                         b'\x28\xb5\x2f\xfd\x00\x48\x01\x00\x00')

    def test_compressobj_large(self):
        chunks = []
        for i in range(255):
            chunks.append(struct.Struct('>B').pack(i) * 16384)

        cctx = zstd.ZstdCompressor(level=3)
        cobj = cctx.compressobj()

        result = cobj.compress(b''.join(chunks)) + cobj.flush()
        self.assertEqual(len(result), 999)
        self.assertEqual(result[0:4], b'\x28\xb5\x2f\xfd')

        params = zstd.get_frame_parameters(result)
        self.assertEqual(params.content_size, 0)
        self.assertEqual(params.window_size, 1048576)
        self.assertEqual(params.dict_id, 0)
        self.assertFalse(params.has_checksum)

    def test_write_checksum(self):
        cctx = zstd.ZstdCompressor(level=1)
        cobj = cctx.compressobj()
        no_checksum = cobj.compress(b'foobar') + cobj.flush()
        cctx = zstd.ZstdCompressor(level=1, write_checksum=True)
        cobj = cctx.compressobj()
        with_checksum = cobj.compress(b'foobar') + cobj.flush()

        no_params = zstd.get_frame_parameters(no_checksum)
        with_params = zstd.get_frame_parameters(with_checksum)
        self.assertEqual(no_params.content_size, 0)
        self.assertEqual(with_params.content_size, 0)
        self.assertEqual(no_params.dict_id, 0)
        self.assertEqual(with_params.dict_id, 0)
        self.assertFalse(no_params.has_checksum)
        self.assertTrue(with_params.has_checksum)

        self.assertEqual(len(with_checksum), len(no_checksum) + 4)

    def test_write_content_size(self):
        cctx = zstd.ZstdCompressor(level=1)
        cobj = cctx.compressobj(size=len(b'foobar' * 256))
        no_size = cobj.compress(b'foobar' * 256) + cobj.flush()
        cctx = zstd.ZstdCompressor(level=1, write_content_size=True)
        cobj = cctx.compressobj(size=len(b'foobar' * 256))
        with_size = cobj.compress(b'foobar' * 256) + cobj.flush()

        no_params = zstd.get_frame_parameters(no_size)
        with_params = zstd.get_frame_parameters(with_size)
        self.assertEqual(no_params.content_size, 0)
        self.assertEqual(with_params.content_size, 1536)
        self.assertEqual(no_params.dict_id, 0)
        self.assertEqual(with_params.dict_id, 0)
        self.assertFalse(no_params.has_checksum)
        self.assertFalse(with_params.has_checksum)

        self.assertEqual(len(with_size), len(no_size) + 1)

    def test_compress_after_finished(self):
        cctx = zstd.ZstdCompressor()
        cobj = cctx.compressobj()

        cobj.compress(b'foo')
        cobj.flush()

        with self.assertRaisesRegexp(zstd.ZstdError, 'cannot call compress\(\) after compressor'):
            cobj.compress(b'foo')

        with self.assertRaisesRegexp(zstd.ZstdError, 'compressor object already finished'):
            cobj.flush()

    def test_flush_block_repeated(self):
        cctx = zstd.ZstdCompressor(level=1)
        cobj = cctx.compressobj()

        self.assertEqual(cobj.compress(b'foo'), b'')
        self.assertEqual(cobj.flush(zstd.COMPRESSOBJ_FLUSH_BLOCK),
                         b'\x28\xb5\x2f\xfd\x00\x48\x18\x00\x00foo')
        self.assertEqual(cobj.compress(b'bar'), b'')
        # 3 byte header plus content.
        self.assertEqual(cobj.flush(), b'\x19\x00\x00bar')

    def test_flush_empty_block(self):
        cctx = zstd.ZstdCompressor(write_checksum=True)
        cobj = cctx.compressobj()

        cobj.compress(b'foobar')
        cobj.flush(zstd.COMPRESSOBJ_FLUSH_BLOCK)
        # No-op if no block is active (this is internal to zstd).
        self.assertEqual(cobj.flush(zstd.COMPRESSOBJ_FLUSH_BLOCK), b'')

        trailing = cobj.flush()
        # 3 bytes block header + 4 bytes frame checksum
        self.assertEqual(len(trailing), 7)
        header = trailing[0:3]
        self.assertEqual(header, b'\x01\x00\x00')

    def test_multithreaded(self):
        source = io.BytesIO()
        source.write(b'a' * 1048576)
        source.write(b'b' * 1048576)
        source.write(b'c' * 1048576)
        source.seek(0)

        cctx = zstd.ZstdCompressor(level=1, threads=2)
        cobj = cctx.compressobj()

        chunks = []
        while True:
            d = source.read(8192)
            if not d:
                break

            chunks.append(cobj.compress(d))

        chunks.append(cobj.flush())

        compressed = b''.join(chunks)

        self.assertEqual(len(compressed), 295)


@make_cffi
class TestCompressor_copy_stream(unittest.TestCase):
    def test_no_read(self):
        source = object()
        dest = io.BytesIO()

        cctx = zstd.ZstdCompressor()
        with self.assertRaises(ValueError):
            cctx.copy_stream(source, dest)

    def test_no_write(self):
        source = io.BytesIO()
        dest = object()

        cctx = zstd.ZstdCompressor()
        with self.assertRaises(ValueError):
            cctx.copy_stream(source, dest)

    def test_empty(self):
        source = io.BytesIO()
        dest = io.BytesIO()

        cctx = zstd.ZstdCompressor(level=1)
        r, w = cctx.copy_stream(source, dest)
        self.assertEqual(int(r), 0)
        self.assertEqual(w, 9)

        self.assertEqual(dest.getvalue(),
                         b'\x28\xb5\x2f\xfd\x00\x48\x01\x00\x00')

    def test_large_data(self):
        source = io.BytesIO()
        for i in range(255):
            source.write(struct.Struct('>B').pack(i) * 16384)
        source.seek(0)

        dest = io.BytesIO()
        cctx = zstd.ZstdCompressor()
        r, w = cctx.copy_stream(source, dest)

        self.assertEqual(r, 255 * 16384)
        self.assertEqual(w, 999)

        params = zstd.get_frame_parameters(dest.getvalue())
        self.assertEqual(params.content_size, 0)
        self.assertEqual(params.window_size, 1048576)
        self.assertEqual(params.dict_id, 0)
        self.assertFalse(params.has_checksum)

    def test_write_checksum(self):
        source = io.BytesIO(b'foobar')
        no_checksum = io.BytesIO()

        cctx = zstd.ZstdCompressor(level=1)
        cctx.copy_stream(source, no_checksum)

        source.seek(0)
        with_checksum = io.BytesIO()
        cctx = zstd.ZstdCompressor(level=1, write_checksum=True)
        cctx.copy_stream(source, with_checksum)

        self.assertEqual(len(with_checksum.getvalue()),
                         len(no_checksum.getvalue()) + 4)

        no_params = zstd.get_frame_parameters(no_checksum.getvalue())
        with_params = zstd.get_frame_parameters(with_checksum.getvalue())
        self.assertEqual(no_params.content_size, 0)
        self.assertEqual(with_params.content_size, 0)
        self.assertEqual(no_params.dict_id, 0)
        self.assertEqual(with_params.dict_id, 0)
        self.assertFalse(no_params.has_checksum)
        self.assertTrue(with_params.has_checksum)

    def test_write_content_size(self):
        source = io.BytesIO(b'foobar' * 256)
        no_size = io.BytesIO()

        cctx = zstd.ZstdCompressor(level=1)
        cctx.copy_stream(source, no_size)

        source.seek(0)
        with_size = io.BytesIO()
        cctx = zstd.ZstdCompressor(level=1, write_content_size=True)
        cctx.copy_stream(source, with_size)

        # Source content size is unknown, so no content size written.
        self.assertEqual(len(with_size.getvalue()),
                         len(no_size.getvalue()))

        source.seek(0)
        with_size = io.BytesIO()
        cctx.copy_stream(source, with_size, size=len(source.getvalue()))

        # We specified source size, so content size header is present.
        self.assertEqual(len(with_size.getvalue()),
                         len(no_size.getvalue()) + 1)

        no_params = zstd.get_frame_parameters(no_size.getvalue())
        with_params = zstd.get_frame_parameters(with_size.getvalue())
        self.assertEqual(no_params.content_size, 0)
        self.assertEqual(with_params.content_size, 1536)
        self.assertEqual(no_params.dict_id, 0)
        self.assertEqual(with_params.dict_id, 0)
        self.assertFalse(no_params.has_checksum)
        self.assertFalse(with_params.has_checksum)

    def test_read_write_size(self):
        source = OpCountingBytesIO(b'foobarfoobar')
        dest = OpCountingBytesIO()
        cctx = zstd.ZstdCompressor()
        r, w = cctx.copy_stream(source, dest, read_size=1, write_size=1)

        self.assertEqual(r, len(source.getvalue()))
        self.assertEqual(w, 21)
        self.assertEqual(source._read_count, len(source.getvalue()) + 1)
        self.assertEqual(dest._write_count, len(dest.getvalue()))

    def test_multithreaded(self):
        source = io.BytesIO()
        source.write(b'a' * 1048576)
        source.write(b'b' * 1048576)
        source.write(b'c' * 1048576)
        source.seek(0)

        dest = io.BytesIO()
        cctx = zstd.ZstdCompressor(threads=2)
        r, w = cctx.copy_stream(source, dest)
        self.assertEqual(r, 3145728)
        self.assertEqual(w, 295)

        params = zstd.get_frame_parameters(dest.getvalue())
        self.assertEqual(params.content_size, 0)
        self.assertEqual(params.dict_id, 0)
        self.assertFalse(params.has_checksum)

        # Writing content size and checksum works.
        cctx = zstd.ZstdCompressor(threads=2, write_content_size=True,
                                   write_checksum=True)
        dest = io.BytesIO()
        source.seek(0)
        cctx.copy_stream(source, dest, size=len(source.getvalue()))

        params = zstd.get_frame_parameters(dest.getvalue())
        self.assertEqual(params.content_size, 3145728)
        self.assertEqual(params.dict_id, 0)
        self.assertTrue(params.has_checksum)


def compress(data, level):
    buffer = io.BytesIO()
    cctx = zstd.ZstdCompressor(level=level)
    with cctx.write_to(buffer) as compressor:
        compressor.write(data)
    return buffer.getvalue()


@make_cffi
class TestCompressor_write_to(unittest.TestCase):
    def test_empty(self):
        result = compress(b'', 1)
        self.assertEqual(result, b'\x28\xb5\x2f\xfd\x00\x48\x01\x00\x00')

        params = zstd.get_frame_parameters(result)
        self.assertEqual(params.content_size, 0)
        self.assertEqual(params.window_size, 524288)
        self.assertEqual(params.dict_id, 0)
        self.assertFalse(params.has_checksum)

    def test_multiple_compress(self):
        buffer = io.BytesIO()
        cctx = zstd.ZstdCompressor(level=5)
        with cctx.write_to(buffer) as compressor:
            self.assertEqual(compressor.write(b'foo'), 0)
            self.assertEqual(compressor.write(b'bar'), 0)
            self.assertEqual(compressor.write(b'x' * 8192), 0)

        result = buffer.getvalue()
        self.assertEqual(result,
                         b'\x28\xb5\x2f\xfd\x00\x50\x75\x00\x00\x38\x66\x6f'
                         b'\x6f\x62\x61\x72\x78\x01\x00\xfc\xdf\x03\x23')

    def test_dictionary(self):
        samples = []
        for i in range(128):
            samples.append(b'foo' * 64)
            samples.append(b'bar' * 64)
            samples.append(b'foobar' * 64)

        d = zstd.train_dictionary(8192, samples)

        buffer = io.BytesIO()
        cctx = zstd.ZstdCompressor(level=9, dict_data=d)
        with cctx.write_to(buffer) as compressor:
            self.assertEqual(compressor.write(b'foo'), 0)
            self.assertEqual(compressor.write(b'bar'), 0)
            self.assertEqual(compressor.write(b'foo' * 16384), 634)

        compressed = buffer.getvalue()

        params = zstd.get_frame_parameters(compressed)
        self.assertEqual(params.content_size, 0)
        self.assertEqual(params.window_size, 1024)
        self.assertEqual(params.dict_id, d.dict_id())
        self.assertFalse(params.has_checksum)

        self.assertEqual(compressed[0:32],
                         b'\x28\xb5\x2f\xfd\x03\x00\x55\x7b\x6b\x5e\x54\x00'
                         b'\x00\x00\x02\xfc\xf4\xa5\xba\x23\x3f\x85\xb3\x54'
                         b'\x00\x00\x18\x6f\x6f\x66\x01\x00')

        h = hashlib.sha1(compressed).hexdigest()
        self.assertEqual(h, '1c5bcd25181bcd8c1a73ea8773323e0056129f92')

    def test_compression_params(self):
        params = zstd.CompressionParameters(20, 6, 12, 5, 4, 10, zstd.STRATEGY_FAST)

        buffer = io.BytesIO()
        cctx = zstd.ZstdCompressor(compression_params=params)
        with cctx.write_to(buffer) as compressor:
            self.assertEqual(compressor.write(b'foo'), 0)
            self.assertEqual(compressor.write(b'bar'), 0)
            self.assertEqual(compressor.write(b'foobar' * 16384), 0)

        compressed = buffer.getvalue()

        params = zstd.get_frame_parameters(compressed)
        self.assertEqual(params.content_size, 0)
        self.assertEqual(params.window_size, 1048576)
        self.assertEqual(params.dict_id, 0)
        self.assertFalse(params.has_checksum)

        h = hashlib.sha1(compressed).hexdigest()
        self.assertEqual(h, '1ae31f270ed7de14235221a604b31ecd517ebd99')

    def test_write_checksum(self):
        no_checksum = io.BytesIO()
        cctx = zstd.ZstdCompressor(level=1)
        with cctx.write_to(no_checksum) as compressor:
            self.assertEqual(compressor.write(b'foobar'), 0)

        with_checksum = io.BytesIO()
        cctx = zstd.ZstdCompressor(level=1, write_checksum=True)
        with cctx.write_to(with_checksum) as compressor:
            self.assertEqual(compressor.write(b'foobar'), 0)

        no_params = zstd.get_frame_parameters(no_checksum.getvalue())
        with_params = zstd.get_frame_parameters(with_checksum.getvalue())
        self.assertEqual(no_params.content_size, 0)
        self.assertEqual(with_params.content_size, 0)
        self.assertEqual(no_params.dict_id, 0)
        self.assertEqual(with_params.dict_id, 0)
        self.assertFalse(no_params.has_checksum)
        self.assertTrue(with_params.has_checksum)

        self.assertEqual(len(with_checksum.getvalue()),
                         len(no_checksum.getvalue()) + 4)

    def test_write_content_size(self):
        no_size = io.BytesIO()
        cctx = zstd.ZstdCompressor(level=1)
        with cctx.write_to(no_size) as compressor:
            self.assertEqual(compressor.write(b'foobar' * 256), 0)

        with_size = io.BytesIO()
        cctx = zstd.ZstdCompressor(level=1, write_content_size=True)
        with cctx.write_to(with_size) as compressor:
            self.assertEqual(compressor.write(b'foobar' * 256), 0)

        # Source size is not known in streaming mode, so header not
        # written.
        self.assertEqual(len(with_size.getvalue()),
                         len(no_size.getvalue()))

        # Declaring size will write the header.
        with_size = io.BytesIO()
        with cctx.write_to(with_size, size=len(b'foobar' * 256)) as compressor:
            self.assertEqual(compressor.write(b'foobar' * 256), 0)

        no_params = zstd.get_frame_parameters(no_size.getvalue())
        with_params = zstd.get_frame_parameters(with_size.getvalue())
        self.assertEqual(no_params.content_size, 0)
        self.assertEqual(with_params.content_size, 1536)
        self.assertEqual(no_params.dict_id, 0)
        self.assertEqual(with_params.dict_id, 0)
        self.assertFalse(no_params.has_checksum)
        self.assertFalse(with_params.has_checksum)

        self.assertEqual(len(with_size.getvalue()),
                         len(no_size.getvalue()) + 1)

    def test_no_dict_id(self):
        samples = []
        for i in range(128):
            samples.append(b'foo' * 64)
            samples.append(b'bar' * 64)
            samples.append(b'foobar' * 64)

        d = zstd.train_dictionary(1024, samples)

        with_dict_id = io.BytesIO()
        cctx = zstd.ZstdCompressor(level=1, dict_data=d)
        with cctx.write_to(with_dict_id) as compressor:
            self.assertEqual(compressor.write(b'foobarfoobar'), 0)

        cctx = zstd.ZstdCompressor(level=1, dict_data=d, write_dict_id=False)
        no_dict_id = io.BytesIO()
        with cctx.write_to(no_dict_id) as compressor:
            self.assertEqual(compressor.write(b'foobarfoobar'), 0)

        no_params = zstd.get_frame_parameters(no_dict_id.getvalue())
        with_params = zstd.get_frame_parameters(with_dict_id.getvalue())
        self.assertEqual(no_params.content_size, 0)
        self.assertEqual(with_params.content_size, 0)
        self.assertEqual(no_params.dict_id, 0)
        self.assertEqual(with_params.dict_id, d.dict_id())
        self.assertFalse(no_params.has_checksum)
        self.assertFalse(with_params.has_checksum)

        self.assertEqual(len(with_dict_id.getvalue()),
                         len(no_dict_id.getvalue()) + 4)

    def test_memory_size(self):
        cctx = zstd.ZstdCompressor(level=3)
        buffer = io.BytesIO()
        with cctx.write_to(buffer) as compressor:
            size = compressor.memory_size()

        self.assertGreater(size, 100000)

    def test_write_size(self):
        cctx = zstd.ZstdCompressor(level=3)
        dest = OpCountingBytesIO()
        with cctx.write_to(dest, write_size=1) as compressor:
            self.assertEqual(compressor.write(b'foo'), 0)
            self.assertEqual(compressor.write(b'bar'), 0)
            self.assertEqual(compressor.write(b'foobar'), 0)

        self.assertEqual(len(dest.getvalue()), dest._write_count)

    def test_flush_repeated(self):
        cctx = zstd.ZstdCompressor(level=3)
        dest = OpCountingBytesIO()
        with cctx.write_to(dest) as compressor:
            self.assertEqual(compressor.write(b'foo'), 0)
            self.assertEqual(dest._write_count, 0)
            self.assertEqual(compressor.flush(), 12)
            self.assertEqual(dest._write_count, 1)
            self.assertEqual(compressor.write(b'bar'), 0)
            self.assertEqual(dest._write_count, 1)
            self.assertEqual(compressor.flush(), 6)
            self.assertEqual(dest._write_count, 2)
            self.assertEqual(compressor.write(b'baz'), 0)

        self.assertEqual(dest._write_count, 3)

    def test_flush_empty_block(self):
        cctx = zstd.ZstdCompressor(level=3, write_checksum=True)
        dest = OpCountingBytesIO()
        with cctx.write_to(dest) as compressor:
            self.assertEqual(compressor.write(b'foobar' * 8192), 0)
            count = dest._write_count
            offset = dest.tell()
            self.assertEqual(compressor.flush(), 23)
            self.assertGreater(dest._write_count, count)
            self.assertGreater(dest.tell(), offset)
            offset = dest.tell()
            # Ending the write here should cause an empty block to be written
            # to denote end of frame.

        trailing = dest.getvalue()[offset:]
        # 3 bytes block header + 4 bytes frame checksum
        self.assertEqual(len(trailing), 7)

        header = trailing[0:3]
        self.assertEqual(header, b'\x01\x00\x00')

    def test_multithreaded(self):
        dest = io.BytesIO()
        cctx = zstd.ZstdCompressor(threads=2)
        with cctx.write_to(dest) as compressor:
            compressor.write(b'a' * 1048576)
            compressor.write(b'b' * 1048576)
            compressor.write(b'c' * 1048576)

        self.assertEqual(len(dest.getvalue()), 295)


@make_cffi
class TestCompressor_read_from(unittest.TestCase):
    def test_type_validation(self):
        cctx = zstd.ZstdCompressor()

        # Object with read() works.
        for chunk in cctx.read_from(io.BytesIO()):
            pass

        # Buffer protocol works.
        for chunk in cctx.read_from(b'foobar'):
            pass

        with self.assertRaisesRegexp(ValueError, 'must pass an object with a read'):
            for chunk in cctx.read_from(True):
                pass

    def test_read_empty(self):
        cctx = zstd.ZstdCompressor(level=1)

        source = io.BytesIO()
        it = cctx.read_from(source)
        chunks = list(it)
        self.assertEqual(len(chunks), 1)
        compressed = b''.join(chunks)
        self.assertEqual(compressed, b'\x28\xb5\x2f\xfd\x00\x48\x01\x00\x00')

        # And again with the buffer protocol.
        it = cctx.read_from(b'')
        chunks = list(it)
        self.assertEqual(len(chunks), 1)
        compressed2 = b''.join(chunks)
        self.assertEqual(compressed2, compressed)

    def test_read_large(self):
        cctx = zstd.ZstdCompressor(level=1)

        source = io.BytesIO()
        source.write(b'f' * zstd.COMPRESSION_RECOMMENDED_INPUT_SIZE)
        source.write(b'o')
        source.seek(0)

        # Creating an iterator should not perform any compression until
        # first read.
        it = cctx.read_from(source, size=len(source.getvalue()))
        self.assertEqual(source.tell(), 0)

        # We should have exactly 2 output chunks.
        chunks = []
        chunk = next(it)
        self.assertIsNotNone(chunk)
        self.assertEqual(source.tell(), zstd.COMPRESSION_RECOMMENDED_INPUT_SIZE)
        chunks.append(chunk)
        chunk = next(it)
        self.assertIsNotNone(chunk)
        chunks.append(chunk)

        self.assertEqual(source.tell(), len(source.getvalue()))

        with self.assertRaises(StopIteration):
            next(it)

        # And again for good measure.
        with self.assertRaises(StopIteration):
            next(it)

        # We should get the same output as the one-shot compression mechanism.
        self.assertEqual(b''.join(chunks), cctx.compress(source.getvalue()))

        params = zstd.get_frame_parameters(b''.join(chunks))
        self.assertEqual(params.content_size, 0)
        self.assertEqual(params.window_size, 262144)
        self.assertEqual(params.dict_id, 0)
        self.assertFalse(params.has_checksum)

        # Now check the buffer protocol.
        it = cctx.read_from(source.getvalue())
        chunks = list(it)
        self.assertEqual(len(chunks), 2)
        self.assertEqual(b''.join(chunks), cctx.compress(source.getvalue()))

    def test_read_write_size(self):
        source = OpCountingBytesIO(b'foobarfoobar')
        cctx = zstd.ZstdCompressor(level=3)
        for chunk in cctx.read_from(source, read_size=1, write_size=1):
            self.assertEqual(len(chunk), 1)

        self.assertEqual(source._read_count, len(source.getvalue()) + 1)

    def test_multithreaded(self):
        source = io.BytesIO()
        source.write(b'a' * 1048576)
        source.write(b'b' * 1048576)
        source.write(b'c' * 1048576)
        source.seek(0)

        cctx = zstd.ZstdCompressor(threads=2)

        compressed = b''.join(cctx.read_from(source))
        self.assertEqual(len(compressed), 295)


class TestCompressor_multi_compress_to_buffer(unittest.TestCase):
    def test_multithreaded_unsupported(self):
        cctx = zstd.ZstdCompressor(threads=2)

        with self.assertRaisesRegexp(zstd.ZstdError, 'function cannot be called on ZstdCompressor configured for multi-threaded compression'):
            cctx.multi_compress_to_buffer([b'foo'])

    def test_invalid_inputs(self):
        cctx = zstd.ZstdCompressor()

        with self.assertRaises(TypeError):
            cctx.multi_compress_to_buffer(True)

        with self.assertRaises(TypeError):
            cctx.multi_compress_to_buffer((1, 2))

        with self.assertRaisesRegexp(TypeError, 'item 0 not a bytes like object'):
            cctx.multi_compress_to_buffer([u'foo'])

    def test_empty_input(self):
        cctx = zstd.ZstdCompressor()

        with self.assertRaisesRegexp(ValueError, 'no source elements found'):
            cctx.multi_compress_to_buffer([])

        with self.assertRaisesRegexp(ValueError, 'source elements are empty'):
            cctx.multi_compress_to_buffer([b'', b'', b''])

    def test_list_input(self):
        cctx = zstd.ZstdCompressor(write_content_size=True, write_checksum=True)

        original = [b'foo' * 12, b'bar' * 6]
        frames = [cctx.compress(c) for c in original]
        b = cctx.multi_compress_to_buffer(original)

        self.assertIsInstance(b, zstd.BufferWithSegmentsCollection)

        self.assertEqual(len(b), 2)
        self.assertEqual(b.size(), 44)

        self.assertEqual(b[0].tobytes(), frames[0])
        self.assertEqual(b[1].tobytes(), frames[1])

    def test_buffer_with_segments_input(self):
        cctx = zstd.ZstdCompressor(write_content_size=True, write_checksum=True)

        original = [b'foo' * 4, b'bar' * 6]
        frames = [cctx.compress(c) for c in original]

        offsets = struct.pack('=QQQQ', 0, len(original[0]),
                                       len(original[0]), len(original[1]))
        segments = zstd.BufferWithSegments(b''.join(original), offsets)

        result = cctx.multi_compress_to_buffer(segments)

        self.assertEqual(len(result), 2)
        self.assertEqual(result.size(), 47)

        self.assertEqual(result[0].tobytes(), frames[0])
        self.assertEqual(result[1].tobytes(), frames[1])

    def test_buffer_with_segments_collection_input(self):
        cctx = zstd.ZstdCompressor(write_content_size=True, write_checksum=True)

        original = [
            b'foo1',
            b'foo2' * 2,
            b'foo3' * 3,
            b'foo4' * 4,
            b'foo5' * 5,
        ]

        frames = [cctx.compress(c) for c in original]

        b = b''.join([original[0], original[1]])
        b1 = zstd.BufferWithSegments(b, struct.pack('=QQQQ',
                                                    0, len(original[0]),
                                                    len(original[0]), len(original[1])))
        b = b''.join([original[2], original[3], original[4]])
        b2 = zstd.BufferWithSegments(b, struct.pack('=QQQQQQ',
                                                    0, len(original[2]),
                                                    len(original[2]), len(original[3]),
                                                    len(original[2]) + len(original[3]), len(original[4])))

        c = zstd.BufferWithSegmentsCollection(b1, b2)

        result = cctx.multi_compress_to_buffer(c)

        self.assertEqual(len(result), len(frames))

        for i, frame in enumerate(frames):
            self.assertEqual(result[i].tobytes(), frame)

    def test_multiple_threads(self):
        # threads argument will cause multi-threaded ZSTD APIs to be used, which will
        # make output different.
        refcctx = zstd.ZstdCompressor(write_content_size=True, write_checksum=True)
        reference = [refcctx.compress(b'x' * 64), refcctx.compress(b'y' * 64)]

        cctx = zstd.ZstdCompressor(write_content_size=True, write_checksum=True)

        frames = []
        frames.extend(b'x' * 64 for i in range(256))
        frames.extend(b'y' * 64 for i in range(256))

        result = cctx.multi_compress_to_buffer(frames, threads=-1)

        self.assertEqual(len(result), 512)
        for i in range(512):
            if i < 256:
                self.assertEqual(result[i].tobytes(), reference[0])
            else:
                self.assertEqual(result[i].tobytes(), reference[1])

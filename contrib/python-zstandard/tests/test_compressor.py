import hashlib
import io
import struct
import sys

try:
    import unittest2 as unittest
except ImportError:
    import unittest

import zstd

from .common import OpCountingBytesIO


if sys.version_info[0] >= 3:
    next = lambda it: it.__next__()
else:
    next = lambda it: it.next()


class TestCompressor(unittest.TestCase):
    def test_level_bounds(self):
        with self.assertRaises(ValueError):
            zstd.ZstdCompressor(level=0)

        with self.assertRaises(ValueError):
            zstd.ZstdCompressor(level=23)


class TestCompressor_compress(unittest.TestCase):
    def test_compress_empty(self):
        cctx = zstd.ZstdCompressor(level=1)
        cctx.compress(b'')

        cctx = zstd.ZstdCompressor(level=22)
        cctx.compress(b'')

    def test_compress_empty(self):
        cctx = zstd.ZstdCompressor(level=1)
        self.assertEqual(cctx.compress(b''),
                         b'\x28\xb5\x2f\xfd\x00\x48\x01\x00\x00')

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

    def test_write_checksum(self):
        cctx = zstd.ZstdCompressor(level=1)
        no_checksum = cctx.compress(b'foobar')
        cctx = zstd.ZstdCompressor(level=1, write_checksum=True)
        with_checksum = cctx.compress(b'foobar')

        self.assertEqual(len(with_checksum), len(no_checksum) + 4)

    def test_write_content_size(self):
        cctx = zstd.ZstdCompressor(level=1)
        no_size = cctx.compress(b'foobar' * 256)
        cctx = zstd.ZstdCompressor(level=1, write_content_size=True)
        with_size = cctx.compress(b'foobar' * 256)

        self.assertEqual(len(with_size), len(no_size) + 1)

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

    def test_write_checksum(self):
        cctx = zstd.ZstdCompressor(level=1)
        cobj = cctx.compressobj()
        no_checksum = cobj.compress(b'foobar') + cobj.flush()
        cctx = zstd.ZstdCompressor(level=1, write_checksum=True)
        cobj = cctx.compressobj()
        with_checksum = cobj.compress(b'foobar') + cobj.flush()

        self.assertEqual(len(with_checksum), len(no_checksum) + 4)

    def test_write_content_size(self):
        cctx = zstd.ZstdCompressor(level=1)
        cobj = cctx.compressobj(size=len(b'foobar' * 256))
        no_size = cobj.compress(b'foobar' * 256) + cobj.flush()
        cctx = zstd.ZstdCompressor(level=1, write_content_size=True)
        cobj = cctx.compressobj(size=len(b'foobar' * 256))
        with_size = cobj.compress(b'foobar' * 256) + cobj.flush()

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

    def test_read_write_size(self):
        source = OpCountingBytesIO(b'foobarfoobar')
        dest = OpCountingBytesIO()
        cctx = zstd.ZstdCompressor()
        r, w = cctx.copy_stream(source, dest, read_size=1, write_size=1)

        self.assertEqual(r, len(source.getvalue()))
        self.assertEqual(w, 21)
        self.assertEqual(source._read_count, len(source.getvalue()) + 1)
        self.assertEqual(dest._write_count, len(dest.getvalue()))


def compress(data, level):
    buffer = io.BytesIO()
    cctx = zstd.ZstdCompressor(level=level)
    with cctx.write_to(buffer) as compressor:
        compressor.write(data)
    return buffer.getvalue()


class TestCompressor_write_to(unittest.TestCase):
    def test_empty(self):
        self.assertEqual(compress(b'', 1),
                         b'\x28\xb5\x2f\xfd\x00\x48\x01\x00\x00')

    def test_multiple_compress(self):
        buffer = io.BytesIO()
        cctx = zstd.ZstdCompressor(level=5)
        with cctx.write_to(buffer) as compressor:
            compressor.write(b'foo')
            compressor.write(b'bar')
            compressor.write(b'x' * 8192)

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
            compressor.write(b'foo')
            compressor.write(b'bar')
            compressor.write(b'foo' * 16384)

        compressed = buffer.getvalue()
        h = hashlib.sha1(compressed).hexdigest()
        self.assertEqual(h, '1c5bcd25181bcd8c1a73ea8773323e0056129f92')

    def test_compression_params(self):
        params = zstd.CompressionParameters(20, 6, 12, 5, 4, 10, zstd.STRATEGY_FAST)

        buffer = io.BytesIO()
        cctx = zstd.ZstdCompressor(compression_params=params)
        with cctx.write_to(buffer) as compressor:
            compressor.write(b'foo')
            compressor.write(b'bar')
            compressor.write(b'foobar' * 16384)

        compressed = buffer.getvalue()
        h = hashlib.sha1(compressed).hexdigest()
        self.assertEqual(h, '1ae31f270ed7de14235221a604b31ecd517ebd99')

    def test_write_checksum(self):
        no_checksum = io.BytesIO()
        cctx = zstd.ZstdCompressor(level=1)
        with cctx.write_to(no_checksum) as compressor:
            compressor.write(b'foobar')

        with_checksum = io.BytesIO()
        cctx = zstd.ZstdCompressor(level=1, write_checksum=True)
        with cctx.write_to(with_checksum) as compressor:
            compressor.write(b'foobar')

        self.assertEqual(len(with_checksum.getvalue()),
                         len(no_checksum.getvalue()) + 4)

    def test_write_content_size(self):
        no_size = io.BytesIO()
        cctx = zstd.ZstdCompressor(level=1)
        with cctx.write_to(no_size) as compressor:
            compressor.write(b'foobar' * 256)

        with_size = io.BytesIO()
        cctx = zstd.ZstdCompressor(level=1, write_content_size=True)
        with cctx.write_to(with_size) as compressor:
            compressor.write(b'foobar' * 256)

        # Source size is not known in streaming mode, so header not
        # written.
        self.assertEqual(len(with_size.getvalue()),
                         len(no_size.getvalue()))

        # Declaring size will write the header.
        with_size = io.BytesIO()
        with cctx.write_to(with_size, size=len(b'foobar' * 256)) as compressor:
            compressor.write(b'foobar' * 256)

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
            compressor.write(b'foobarfoobar')

        cctx = zstd.ZstdCompressor(level=1, dict_data=d, write_dict_id=False)
        no_dict_id = io.BytesIO()
        with cctx.write_to(no_dict_id) as compressor:
            compressor.write(b'foobarfoobar')

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
            compressor.write(b'foo')
            compressor.write(b'bar')
            compressor.write(b'foobar')

        self.assertEqual(len(dest.getvalue()), dest._write_count)

    def test_flush_repeated(self):
        cctx = zstd.ZstdCompressor(level=3)
        dest = OpCountingBytesIO()
        with cctx.write_to(dest) as compressor:
            compressor.write(b'foo')
            self.assertEqual(dest._write_count, 0)
            compressor.flush()
            self.assertEqual(dest._write_count, 1)
            compressor.write(b'bar')
            self.assertEqual(dest._write_count, 1)
            compressor.flush()
            self.assertEqual(dest._write_count, 2)
            compressor.write(b'baz')

        self.assertEqual(dest._write_count, 3)

    def test_flush_empty_block(self):
        cctx = zstd.ZstdCompressor(level=3, write_checksum=True)
        dest = OpCountingBytesIO()
        with cctx.write_to(dest) as compressor:
            compressor.write(b'foobar' * 8192)
            count = dest._write_count
            offset = dest.tell()
            compressor.flush()
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


class TestCompressor_read_from(unittest.TestCase):
    def test_type_validation(self):
        cctx = zstd.ZstdCompressor()

        # Object with read() works.
        cctx.read_from(io.BytesIO())

        # Buffer protocol works.
        cctx.read_from(b'foobar')

        with self.assertRaisesRegexp(ValueError, 'must pass an object with a read'):
            cctx.read_from(True)

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

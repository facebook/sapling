import io
import random
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


@make_cffi
class TestDecompressor_decompress(unittest.TestCase):
    def test_empty_input(self):
        dctx = zstd.ZstdDecompressor()

        with self.assertRaisesRegexp(zstd.ZstdError, 'input data invalid'):
            dctx.decompress(b'')

    def test_invalid_input(self):
        dctx = zstd.ZstdDecompressor()

        with self.assertRaisesRegexp(zstd.ZstdError, 'input data invalid'):
            dctx.decompress(b'foobar')

    def test_no_content_size_in_frame(self):
        cctx = zstd.ZstdCompressor(write_content_size=False)
        compressed = cctx.compress(b'foobar')

        dctx = zstd.ZstdDecompressor()
        with self.assertRaisesRegexp(zstd.ZstdError, 'input data invalid'):
            dctx.decompress(compressed)

    def test_content_size_present(self):
        cctx = zstd.ZstdCompressor(write_content_size=True)
        compressed = cctx.compress(b'foobar')

        dctx = zstd.ZstdDecompressor()
        decompressed = dctx.decompress(compressed)
        self.assertEqual(decompressed, b'foobar')

    def test_max_output_size(self):
        cctx = zstd.ZstdCompressor(write_content_size=False)
        source = b'foobar' * 256
        compressed = cctx.compress(source)

        dctx = zstd.ZstdDecompressor()
        # Will fit into buffer exactly the size of input.
        decompressed = dctx.decompress(compressed, max_output_size=len(source))
        self.assertEqual(decompressed, source)

        # Input size - 1 fails
        with self.assertRaisesRegexp(zstd.ZstdError, 'Destination buffer is too small'):
            dctx.decompress(compressed, max_output_size=len(source) - 1)

        # Input size + 1 works
        decompressed = dctx.decompress(compressed, max_output_size=len(source) + 1)
        self.assertEqual(decompressed, source)

        # A much larger buffer works.
        decompressed = dctx.decompress(compressed, max_output_size=len(source) * 64)
        self.assertEqual(decompressed, source)

    def test_stupidly_large_output_buffer(self):
        cctx = zstd.ZstdCompressor(write_content_size=False)
        compressed = cctx.compress(b'foobar' * 256)
        dctx = zstd.ZstdDecompressor()

        # Will get OverflowError on some Python distributions that can't
        # handle really large integers.
        with self.assertRaises((MemoryError, OverflowError)):
            dctx.decompress(compressed, max_output_size=2**62)

    def test_dictionary(self):
        samples = []
        for i in range(128):
            samples.append(b'foo' * 64)
            samples.append(b'bar' * 64)
            samples.append(b'foobar' * 64)

        d = zstd.train_dictionary(8192, samples)

        orig = b'foobar' * 16384
        cctx = zstd.ZstdCompressor(level=1, dict_data=d, write_content_size=True)
        compressed = cctx.compress(orig)

        dctx = zstd.ZstdDecompressor(dict_data=d)
        decompressed = dctx.decompress(compressed)

        self.assertEqual(decompressed, orig)

    def test_dictionary_multiple(self):
        samples = []
        for i in range(128):
            samples.append(b'foo' * 64)
            samples.append(b'bar' * 64)
            samples.append(b'foobar' * 64)

        d = zstd.train_dictionary(8192, samples)

        sources = (b'foobar' * 8192, b'foo' * 8192, b'bar' * 8192)
        compressed = []
        cctx = zstd.ZstdCompressor(level=1, dict_data=d, write_content_size=True)
        for source in sources:
            compressed.append(cctx.compress(source))

        dctx = zstd.ZstdDecompressor(dict_data=d)
        for i in range(len(sources)):
            decompressed = dctx.decompress(compressed[i])
            self.assertEqual(decompressed, sources[i])


@make_cffi
class TestDecompressor_copy_stream(unittest.TestCase):
    def test_no_read(self):
        source = object()
        dest = io.BytesIO()

        dctx = zstd.ZstdDecompressor()
        with self.assertRaises(ValueError):
            dctx.copy_stream(source, dest)

    def test_no_write(self):
        source = io.BytesIO()
        dest = object()

        dctx = zstd.ZstdDecompressor()
        with self.assertRaises(ValueError):
            dctx.copy_stream(source, dest)

    def test_empty(self):
        source = io.BytesIO()
        dest = io.BytesIO()

        dctx = zstd.ZstdDecompressor()
        # TODO should this raise an error?
        r, w = dctx.copy_stream(source, dest)

        self.assertEqual(r, 0)
        self.assertEqual(w, 0)
        self.assertEqual(dest.getvalue(), b'')

    def test_large_data(self):
        source = io.BytesIO()
        for i in range(255):
            source.write(struct.Struct('>B').pack(i) * 16384)
        source.seek(0)

        compressed = io.BytesIO()
        cctx = zstd.ZstdCompressor()
        cctx.copy_stream(source, compressed)

        compressed.seek(0)
        dest = io.BytesIO()
        dctx = zstd.ZstdDecompressor()
        r, w = dctx.copy_stream(compressed, dest)

        self.assertEqual(r, len(compressed.getvalue()))
        self.assertEqual(w, len(source.getvalue()))

    def test_read_write_size(self):
        source = OpCountingBytesIO(zstd.ZstdCompressor().compress(
            b'foobarfoobar'))

        dest = OpCountingBytesIO()
        dctx = zstd.ZstdDecompressor()
        r, w = dctx.copy_stream(source, dest, read_size=1, write_size=1)

        self.assertEqual(r, len(source.getvalue()))
        self.assertEqual(w, len(b'foobarfoobar'))
        self.assertEqual(source._read_count, len(source.getvalue()) + 1)
        self.assertEqual(dest._write_count, len(dest.getvalue()))


@make_cffi
class TestDecompressor_decompressobj(unittest.TestCase):
    def test_simple(self):
        data = zstd.ZstdCompressor(level=1).compress(b'foobar')

        dctx = zstd.ZstdDecompressor()
        dobj = dctx.decompressobj()
        self.assertEqual(dobj.decompress(data), b'foobar')

    def test_reuse(self):
        data = zstd.ZstdCompressor(level=1).compress(b'foobar')

        dctx = zstd.ZstdDecompressor()
        dobj = dctx.decompressobj()
        dobj.decompress(data)

        with self.assertRaisesRegexp(zstd.ZstdError, 'cannot use a decompressobj'):
            dobj.decompress(data)


def decompress_via_writer(data):
    buffer = io.BytesIO()
    dctx = zstd.ZstdDecompressor()
    with dctx.write_to(buffer) as decompressor:
        decompressor.write(data)
    return buffer.getvalue()


@make_cffi
class TestDecompressor_write_to(unittest.TestCase):
    def test_empty_roundtrip(self):
        cctx = zstd.ZstdCompressor()
        empty = cctx.compress(b'')
        self.assertEqual(decompress_via_writer(empty), b'')

    def test_large_roundtrip(self):
        chunks = []
        for i in range(255):
            chunks.append(struct.Struct('>B').pack(i) * 16384)
        orig = b''.join(chunks)
        cctx = zstd.ZstdCompressor()
        compressed = cctx.compress(orig)

        self.assertEqual(decompress_via_writer(compressed), orig)

    def test_multiple_calls(self):
        chunks = []
        for i in range(255):
            for j in range(255):
                chunks.append(struct.Struct('>B').pack(j) * i)

        orig = b''.join(chunks)
        cctx = zstd.ZstdCompressor()
        compressed = cctx.compress(orig)

        buffer = io.BytesIO()
        dctx = zstd.ZstdDecompressor()
        with dctx.write_to(buffer) as decompressor:
            pos = 0
            while pos < len(compressed):
                pos2 = pos + 8192
                decompressor.write(compressed[pos:pos2])
                pos += 8192
        self.assertEqual(buffer.getvalue(), orig)

    def test_dictionary(self):
        samples = []
        for i in range(128):
            samples.append(b'foo' * 64)
            samples.append(b'bar' * 64)
            samples.append(b'foobar' * 64)

        d = zstd.train_dictionary(8192, samples)

        orig = b'foobar' * 16384
        buffer = io.BytesIO()
        cctx = zstd.ZstdCompressor(dict_data=d)
        with cctx.write_to(buffer) as compressor:
            self.assertEqual(compressor.write(orig), 1544)

        compressed = buffer.getvalue()
        buffer = io.BytesIO()

        dctx = zstd.ZstdDecompressor(dict_data=d)
        with dctx.write_to(buffer) as decompressor:
            self.assertEqual(decompressor.write(compressed), len(orig))

        self.assertEqual(buffer.getvalue(), orig)

    def test_memory_size(self):
        dctx = zstd.ZstdDecompressor()
        buffer = io.BytesIO()
        with dctx.write_to(buffer) as decompressor:
            size = decompressor.memory_size()

        self.assertGreater(size, 100000)

    def test_write_size(self):
        source = zstd.ZstdCompressor().compress(b'foobarfoobar')
        dest = OpCountingBytesIO()
        dctx = zstd.ZstdDecompressor()
        with dctx.write_to(dest, write_size=1) as decompressor:
            s = struct.Struct('>B')
            for c in source:
                if not isinstance(c, str):
                    c = s.pack(c)
                decompressor.write(c)

        self.assertEqual(dest.getvalue(), b'foobarfoobar')
        self.assertEqual(dest._write_count, len(dest.getvalue()))


@make_cffi
class TestDecompressor_read_from(unittest.TestCase):
    def test_type_validation(self):
        dctx = zstd.ZstdDecompressor()

        # Object with read() works.
        dctx.read_from(io.BytesIO())

        # Buffer protocol works.
        dctx.read_from(b'foobar')

        with self.assertRaisesRegexp(ValueError, 'must pass an object with a read'):
            b''.join(dctx.read_from(True))

    def test_empty_input(self):
        dctx = zstd.ZstdDecompressor()

        source = io.BytesIO()
        it = dctx.read_from(source)
        # TODO this is arguably wrong. Should get an error about missing frame foo.
        with self.assertRaises(StopIteration):
            next(it)

        it = dctx.read_from(b'')
        with self.assertRaises(StopIteration):
            next(it)

    def test_invalid_input(self):
        dctx = zstd.ZstdDecompressor()

        source = io.BytesIO(b'foobar')
        it = dctx.read_from(source)
        with self.assertRaisesRegexp(zstd.ZstdError, 'Unknown frame descriptor'):
            next(it)

        it = dctx.read_from(b'foobar')
        with self.assertRaisesRegexp(zstd.ZstdError, 'Unknown frame descriptor'):
            next(it)

    def test_empty_roundtrip(self):
        cctx = zstd.ZstdCompressor(level=1, write_content_size=False)
        empty = cctx.compress(b'')

        source = io.BytesIO(empty)
        source.seek(0)

        dctx = zstd.ZstdDecompressor()
        it = dctx.read_from(source)

        # No chunks should be emitted since there is no data.
        with self.assertRaises(StopIteration):
            next(it)

        # Again for good measure.
        with self.assertRaises(StopIteration):
            next(it)

    def test_skip_bytes_too_large(self):
        dctx = zstd.ZstdDecompressor()

        with self.assertRaisesRegexp(ValueError, 'skip_bytes must be smaller than read_size'):
            b''.join(dctx.read_from(b'', skip_bytes=1, read_size=1))

        with self.assertRaisesRegexp(ValueError, 'skip_bytes larger than first input chunk'):
            b''.join(dctx.read_from(b'foobar', skip_bytes=10))

    def test_skip_bytes(self):
        cctx = zstd.ZstdCompressor(write_content_size=False)
        compressed = cctx.compress(b'foobar')

        dctx = zstd.ZstdDecompressor()
        output = b''.join(dctx.read_from(b'hdr' + compressed, skip_bytes=3))
        self.assertEqual(output, b'foobar')

    def test_large_output(self):
        source = io.BytesIO()
        source.write(b'f' * zstd.DECOMPRESSION_RECOMMENDED_OUTPUT_SIZE)
        source.write(b'o')
        source.seek(0)

        cctx = zstd.ZstdCompressor(level=1)
        compressed = io.BytesIO(cctx.compress(source.getvalue()))
        compressed.seek(0)

        dctx = zstd.ZstdDecompressor()
        it = dctx.read_from(compressed)

        chunks = []
        chunks.append(next(it))
        chunks.append(next(it))

        with self.assertRaises(StopIteration):
            next(it)

        decompressed = b''.join(chunks)
        self.assertEqual(decompressed, source.getvalue())

        # And again with buffer protocol.
        it = dctx.read_from(compressed.getvalue())
        chunks = []
        chunks.append(next(it))
        chunks.append(next(it))

        with self.assertRaises(StopIteration):
            next(it)

        decompressed = b''.join(chunks)
        self.assertEqual(decompressed, source.getvalue())

    def test_large_input(self):
        bytes = list(struct.Struct('>B').pack(i) for i in range(256))
        compressed = io.BytesIO()
        input_size = 0
        cctx = zstd.ZstdCompressor(level=1)
        with cctx.write_to(compressed) as compressor:
            while True:
                compressor.write(random.choice(bytes))
                input_size += 1

                have_compressed = len(compressed.getvalue()) > zstd.DECOMPRESSION_RECOMMENDED_INPUT_SIZE
                have_raw = input_size > zstd.DECOMPRESSION_RECOMMENDED_OUTPUT_SIZE * 2
                if have_compressed and have_raw:
                    break

        compressed.seek(0)
        self.assertGreater(len(compressed.getvalue()),
                           zstd.DECOMPRESSION_RECOMMENDED_INPUT_SIZE)

        dctx = zstd.ZstdDecompressor()
        it = dctx.read_from(compressed)

        chunks = []
        chunks.append(next(it))
        chunks.append(next(it))
        chunks.append(next(it))

        with self.assertRaises(StopIteration):
            next(it)

        decompressed = b''.join(chunks)
        self.assertEqual(len(decompressed), input_size)

        # And again with buffer protocol.
        it = dctx.read_from(compressed.getvalue())

        chunks = []
        chunks.append(next(it))
        chunks.append(next(it))
        chunks.append(next(it))

        with self.assertRaises(StopIteration):
            next(it)

        decompressed = b''.join(chunks)
        self.assertEqual(len(decompressed), input_size)

    def test_interesting(self):
        # Found this edge case via fuzzing.
        cctx = zstd.ZstdCompressor(level=1)

        source = io.BytesIO()

        compressed = io.BytesIO()
        with cctx.write_to(compressed) as compressor:
            for i in range(256):
                chunk = b'\0' * 1024
                compressor.write(chunk)
                source.write(chunk)

        dctx = zstd.ZstdDecompressor()

        simple = dctx.decompress(compressed.getvalue(),
                                 max_output_size=len(source.getvalue()))
        self.assertEqual(simple, source.getvalue())

        compressed.seek(0)
        streamed = b''.join(dctx.read_from(compressed))
        self.assertEqual(streamed, source.getvalue())

    def test_read_write_size(self):
        source = OpCountingBytesIO(zstd.ZstdCompressor().compress(b'foobarfoobar'))
        dctx = zstd.ZstdDecompressor()
        for chunk in dctx.read_from(source, read_size=1, write_size=1):
            self.assertEqual(len(chunk), 1)

        self.assertEqual(source._read_count, len(source.getvalue()))


@make_cffi
class TestDecompressor_content_dict_chain(unittest.TestCase):
    def test_bad_inputs_simple(self):
        dctx = zstd.ZstdDecompressor()

        with self.assertRaises(TypeError):
            dctx.decompress_content_dict_chain(b'foo')

        with self.assertRaises(TypeError):
            dctx.decompress_content_dict_chain((b'foo', b'bar'))

        with self.assertRaisesRegexp(ValueError, 'empty input chain'):
            dctx.decompress_content_dict_chain([])

        with self.assertRaisesRegexp(ValueError, 'chunk 0 must be bytes'):
            dctx.decompress_content_dict_chain([u'foo'])

        with self.assertRaisesRegexp(ValueError, 'chunk 0 must be bytes'):
            dctx.decompress_content_dict_chain([True])

        with self.assertRaisesRegexp(ValueError, 'chunk 0 is too small to contain a zstd frame'):
            dctx.decompress_content_dict_chain([zstd.FRAME_HEADER])

        with self.assertRaisesRegexp(ValueError, 'chunk 0 is not a valid zstd frame'):
            dctx.decompress_content_dict_chain([b'foo' * 8])

        no_size = zstd.ZstdCompressor().compress(b'foo' * 64)

        with self.assertRaisesRegexp(ValueError, 'chunk 0 missing content size in frame'):
            dctx.decompress_content_dict_chain([no_size])

        # Corrupt first frame.
        frame = zstd.ZstdCompressor(write_content_size=True).compress(b'foo' * 64)
        frame = frame[0:12] + frame[15:]
        with self.assertRaisesRegexp(zstd.ZstdError, 'could not decompress chunk 0'):
            dctx.decompress_content_dict_chain([frame])

    def test_bad_subsequent_input(self):
        initial = zstd.ZstdCompressor(write_content_size=True).compress(b'foo' * 64)

        dctx = zstd.ZstdDecompressor()

        with self.assertRaisesRegexp(ValueError, 'chunk 1 must be bytes'):
            dctx.decompress_content_dict_chain([initial, u'foo'])

        with self.assertRaisesRegexp(ValueError, 'chunk 1 must be bytes'):
            dctx.decompress_content_dict_chain([initial, None])

        with self.assertRaisesRegexp(ValueError, 'chunk 1 is too small to contain a zstd frame'):
            dctx.decompress_content_dict_chain([initial, zstd.FRAME_HEADER])

        with self.assertRaisesRegexp(ValueError, 'chunk 1 is not a valid zstd frame'):
            dctx.decompress_content_dict_chain([initial, b'foo' * 8])

        no_size = zstd.ZstdCompressor().compress(b'foo' * 64)

        with self.assertRaisesRegexp(ValueError, 'chunk 1 missing content size in frame'):
            dctx.decompress_content_dict_chain([initial, no_size])

        # Corrupt second frame.
        cctx = zstd.ZstdCompressor(write_content_size=True, dict_data=zstd.ZstdCompressionDict(b'foo' * 64))
        frame = cctx.compress(b'bar' * 64)
        frame = frame[0:12] + frame[15:]

        with self.assertRaisesRegexp(zstd.ZstdError, 'could not decompress chunk 1'):
            dctx.decompress_content_dict_chain([initial, frame])

    def test_simple(self):
        original = [
            b'foo' * 64,
            b'foobar' * 64,
            b'baz' * 64,
            b'foobaz' * 64,
            b'foobarbaz' * 64,
        ]

        chunks = []
        chunks.append(zstd.ZstdCompressor(write_content_size=True).compress(original[0]))
        for i, chunk in enumerate(original[1:]):
            d = zstd.ZstdCompressionDict(original[i])
            cctx = zstd.ZstdCompressor(dict_data=d, write_content_size=True)
            chunks.append(cctx.compress(chunk))

        for i in range(1, len(original)):
            chain = chunks[0:i]
            expected = original[i - 1]
            dctx = zstd.ZstdDecompressor()
            decompressed = dctx.decompress_content_dict_chain(chain)
            self.assertEqual(decompressed, expected)


# TODO enable for CFFI
class TestDecompressor_multi_decompress_to_buffer(unittest.TestCase):
    def test_invalid_inputs(self):
        dctx = zstd.ZstdDecompressor()

        with self.assertRaises(TypeError):
            dctx.multi_decompress_to_buffer(True)

        with self.assertRaises(TypeError):
            dctx.multi_decompress_to_buffer((1, 2))

        with self.assertRaisesRegexp(TypeError, 'item 0 not a bytes like object'):
            dctx.multi_decompress_to_buffer([u'foo'])

        with self.assertRaisesRegexp(ValueError, 'could not determine decompressed size of item 0'):
            dctx.multi_decompress_to_buffer([b'foobarbaz'])

    def test_list_input(self):
        cctx = zstd.ZstdCompressor(write_content_size=True)

        original = [b'foo' * 4, b'bar' * 6]
        frames = [cctx.compress(d) for d in original]

        dctx = zstd.ZstdDecompressor()
        result = dctx.multi_decompress_to_buffer(frames)

        self.assertEqual(len(result), len(frames))
        self.assertEqual(result.size(), sum(map(len, original)))

        for i, data in enumerate(original):
            self.assertEqual(result[i].tobytes(), data)

        self.assertEqual(result[0].offset, 0)
        self.assertEqual(len(result[0]), 12)
        self.assertEqual(result[1].offset, 12)
        self.assertEqual(len(result[1]), 18)

    def test_list_input_frame_sizes(self):
        cctx = zstd.ZstdCompressor(write_content_size=False)

        original = [b'foo' * 4, b'bar' * 6, b'baz' * 8]
        frames = [cctx.compress(d) for d in original]
        sizes = struct.pack('=' + 'Q' * len(original), *map(len, original))

        dctx = zstd.ZstdDecompressor()
        result = dctx.multi_decompress_to_buffer(frames, decompressed_sizes=sizes)

        self.assertEqual(len(result), len(frames))
        self.assertEqual(result.size(), sum(map(len, original)))

        for i, data in enumerate(original):
            self.assertEqual(result[i].tobytes(), data)

    def test_buffer_with_segments_input(self):
        cctx = zstd.ZstdCompressor(write_content_size=True)

        original = [b'foo' * 4, b'bar' * 6]
        frames = [cctx.compress(d) for d in original]

        dctx = zstd.ZstdDecompressor()

        segments = struct.pack('=QQQQ', 0, len(frames[0]), len(frames[0]), len(frames[1]))
        b = zstd.BufferWithSegments(b''.join(frames), segments)

        result = dctx.multi_decompress_to_buffer(b)

        self.assertEqual(len(result), len(frames))
        self.assertEqual(result[0].offset, 0)
        self.assertEqual(len(result[0]), 12)
        self.assertEqual(result[1].offset, 12)
        self.assertEqual(len(result[1]), 18)

    def test_buffer_with_segments_sizes(self):
        cctx = zstd.ZstdCompressor(write_content_size=False)
        original = [b'foo' * 4, b'bar' * 6, b'baz' * 8]
        frames = [cctx.compress(d) for d in original]
        sizes = struct.pack('=' + 'Q' * len(original), *map(len, original))

        segments = struct.pack('=QQQQQQ', 0, len(frames[0]),
                               len(frames[0]), len(frames[1]),
                               len(frames[0]) + len(frames[1]), len(frames[2]))
        b = zstd.BufferWithSegments(b''.join(frames), segments)

        dctx = zstd.ZstdDecompressor()
        result = dctx.multi_decompress_to_buffer(b, decompressed_sizes=sizes)

        self.assertEqual(len(result), len(frames))
        self.assertEqual(result.size(), sum(map(len, original)))

        for i, data in enumerate(original):
            self.assertEqual(result[i].tobytes(), data)

    def test_buffer_with_segments_collection_input(self):
        cctx = zstd.ZstdCompressor(write_content_size=True)

        original = [
            b'foo0' * 2,
            b'foo1' * 3,
            b'foo2' * 4,
            b'foo3' * 5,
            b'foo4' * 6,
        ]

        frames = cctx.multi_compress_to_buffer(original)

        # Check round trip.
        dctx = zstd.ZstdDecompressor()
        decompressed = dctx.multi_decompress_to_buffer(frames, threads=3)

        self.assertEqual(len(decompressed), len(original))

        for i, data in enumerate(original):
            self.assertEqual(data, decompressed[i].tobytes())

        # And a manual mode.
        b = b''.join([frames[0].tobytes(), frames[1].tobytes()])
        b1 = zstd.BufferWithSegments(b, struct.pack('=QQQQ',
                                                    0, len(frames[0]),
                                                    len(frames[0]), len(frames[1])))

        b = b''.join([frames[2].tobytes(), frames[3].tobytes(), frames[4].tobytes()])
        b2 = zstd.BufferWithSegments(b, struct.pack('=QQQQQQ',
                                                    0, len(frames[2]),
                                                    len(frames[2]), len(frames[3]),
                                                    len(frames[2]) + len(frames[3]), len(frames[4])))

        c = zstd.BufferWithSegmentsCollection(b1, b2)

        dctx = zstd.ZstdDecompressor()
        decompressed = dctx.multi_decompress_to_buffer(c)

        self.assertEqual(len(decompressed), 5)
        for i in range(5):
            self.assertEqual(decompressed[i].tobytes(), original[i])

    def test_multiple_threads(self):
        cctx = zstd.ZstdCompressor(write_content_size=True)

        frames = []
        frames.extend(cctx.compress(b'x' * 64) for i in range(256))
        frames.extend(cctx.compress(b'y' * 64) for i in range(256))

        dctx = zstd.ZstdDecompressor()
        result = dctx.multi_decompress_to_buffer(frames, threads=-1)

        self.assertEqual(len(result), len(frames))
        self.assertEqual(result.size(), 2 * 64 * 256)
        self.assertEqual(result[0].tobytes(), b'x' * 64)
        self.assertEqual(result[256].tobytes(), b'y' * 64)

    def test_item_failure(self):
        cctx = zstd.ZstdCompressor(write_content_size=True)
        frames = [cctx.compress(b'x' * 128), cctx.compress(b'y' * 128)]

        frames[1] = frames[1] + b'extra'

        dctx = zstd.ZstdDecompressor()

        with self.assertRaisesRegexp(zstd.ZstdError, 'error decompressing item 1: Src size incorrect'):
            dctx.multi_decompress_to_buffer(frames)

        with self.assertRaisesRegexp(zstd.ZstdError, 'error decompressing item 1: Src size incorrect'):
            dctx.multi_decompress_to_buffer(frames, threads=2)

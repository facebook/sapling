import struct

try:
    import unittest2 as unittest
except ImportError:
    import unittest

import zstd

ss = struct.Struct('=QQ')


class TestBufferWithSegments(unittest.TestCase):
    def test_arguments(self):
        with self.assertRaises(TypeError):
            zstd.BufferWithSegments()

        with self.assertRaises(TypeError):
            zstd.BufferWithSegments(b'foo')

        # Segments data should be a multiple of 16.
        with self.assertRaisesRegexp(ValueError, 'segments array size is not a multiple of 16'):
            zstd.BufferWithSegments(b'foo', b'\x00\x00')

    def test_invalid_offset(self):
        with self.assertRaisesRegexp(ValueError, 'offset within segments array references memory'):
            zstd.BufferWithSegments(b'foo', ss.pack(0, 4))

    def test_invalid_getitem(self):
        b = zstd.BufferWithSegments(b'foo', ss.pack(0, 3))

        with self.assertRaisesRegexp(IndexError, 'offset must be non-negative'):
            test = b[-10]

        with self.assertRaisesRegexp(IndexError, 'offset must be less than 1'):
            test = b[1]

        with self.assertRaisesRegexp(IndexError, 'offset must be less than 1'):
            test = b[2]

    def test_single(self):
        b = zstd.BufferWithSegments(b'foo', ss.pack(0, 3))
        self.assertEqual(len(b), 1)
        self.assertEqual(b.size, 3)
        self.assertEqual(b.tobytes(), b'foo')

        self.assertEqual(len(b[0]), 3)
        self.assertEqual(b[0].offset, 0)
        self.assertEqual(b[0].tobytes(), b'foo')

    def test_multiple(self):
        b = zstd.BufferWithSegments(b'foofooxfooxy', b''.join([ss.pack(0, 3),
                                                               ss.pack(3, 4),
                                                               ss.pack(7, 5)]))
        self.assertEqual(len(b), 3)
        self.assertEqual(b.size, 12)
        self.assertEqual(b.tobytes(), b'foofooxfooxy')

        self.assertEqual(b[0].tobytes(), b'foo')
        self.assertEqual(b[1].tobytes(), b'foox')
        self.assertEqual(b[2].tobytes(), b'fooxy')


class TestBufferWithSegmentsCollection(unittest.TestCase):
    def test_empty_constructor(self):
        with self.assertRaisesRegexp(ValueError, 'must pass at least 1 argument'):
            zstd.BufferWithSegmentsCollection()

    def test_argument_validation(self):
        with self.assertRaisesRegexp(TypeError, 'arguments must be BufferWithSegments'):
            zstd.BufferWithSegmentsCollection(None)

        with self.assertRaisesRegexp(TypeError, 'arguments must be BufferWithSegments'):
            zstd.BufferWithSegmentsCollection(zstd.BufferWithSegments(b'foo', ss.pack(0, 3)),
                                              None)

        with self.assertRaisesRegexp(ValueError, 'ZstdBufferWithSegments cannot be empty'):
            zstd.BufferWithSegmentsCollection(zstd.BufferWithSegments(b'', b''))

    def test_length(self):
        b1 = zstd.BufferWithSegments(b'foo', ss.pack(0, 3))
        b2 = zstd.BufferWithSegments(b'barbaz', b''.join([ss.pack(0, 3),
                                                          ss.pack(3, 3)]))

        c = zstd.BufferWithSegmentsCollection(b1)
        self.assertEqual(len(c), 1)
        self.assertEqual(c.size(), 3)

        c = zstd.BufferWithSegmentsCollection(b2)
        self.assertEqual(len(c), 2)
        self.assertEqual(c.size(), 6)

        c = zstd.BufferWithSegmentsCollection(b1, b2)
        self.assertEqual(len(c), 3)
        self.assertEqual(c.size(), 9)

    def test_getitem(self):
        b1 = zstd.BufferWithSegments(b'foo', ss.pack(0, 3))
        b2 = zstd.BufferWithSegments(b'barbaz', b''.join([ss.pack(0, 3),
                                                          ss.pack(3, 3)]))

        c = zstd.BufferWithSegmentsCollection(b1, b2)

        with self.assertRaisesRegexp(IndexError, 'offset must be less than 3'):
            c[3]

        with self.assertRaisesRegexp(IndexError, 'offset must be less than 3'):
            c[4]

        self.assertEqual(c[0].tobytes(), b'foo')
        self.assertEqual(c[1].tobytes(), b'bar')
        self.assertEqual(c[2].tobytes(), b'baz')

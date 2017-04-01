try:
    import unittest2 as unittest
except ImportError:
    import unittest

import zstd

from . common import (
    make_cffi,
)


@make_cffi
class TestCompressionParameters(unittest.TestCase):
    def test_init_bad_arg_type(self):
        with self.assertRaises(TypeError):
            zstd.CompressionParameters()

        with self.assertRaises(TypeError):
            zstd.CompressionParameters(0, 1)

    def test_bounds(self):
        zstd.CompressionParameters(zstd.WINDOWLOG_MIN,
                                   zstd.CHAINLOG_MIN,
                                   zstd.HASHLOG_MIN,
                                   zstd.SEARCHLOG_MIN,
                                   zstd.SEARCHLENGTH_MIN + 1,
                                   zstd.TARGETLENGTH_MIN,
                                   zstd.STRATEGY_FAST)

        zstd.CompressionParameters(zstd.WINDOWLOG_MAX,
                                   zstd.CHAINLOG_MAX,
                                   zstd.HASHLOG_MAX,
                                   zstd.SEARCHLOG_MAX,
                                   zstd.SEARCHLENGTH_MAX - 1,
                                   zstd.TARGETLENGTH_MAX,
                                   zstd.STRATEGY_BTOPT)

    def test_get_compression_parameters(self):
        p = zstd.get_compression_parameters(1)
        self.assertIsInstance(p, zstd.CompressionParameters)

        self.assertEqual(p.window_log, 19)

    def test_members(self):
        p = zstd.CompressionParameters(10, 6, 7, 4, 5, 8, 1)
        self.assertEqual(p.window_log, 10)
        self.assertEqual(p.chain_log, 6)
        self.assertEqual(p.hash_log, 7)
        self.assertEqual(p.search_log, 4)
        self.assertEqual(p.search_length, 5)
        self.assertEqual(p.target_length, 8)
        self.assertEqual(p.strategy, 1)

    def test_estimated_compression_context_size(self):
        p = zstd.CompressionParameters(20, 16, 17,  1,  5, 16, zstd.STRATEGY_DFAST)

        # 32-bit has slightly different values from 64-bit.
        self.assertAlmostEqual(p.estimated_compression_context_size(), 1287076,
                               delta=110)


@make_cffi
class TestFrameParameters(unittest.TestCase):
    def test_invalid_type(self):
        with self.assertRaises(TypeError):
            zstd.get_frame_parameters(None)

        with self.assertRaises(TypeError):
            zstd.get_frame_parameters(u'foobarbaz')

    def test_invalid_input_sizes(self):
        with self.assertRaisesRegexp(zstd.ZstdError, 'not enough data for frame'):
            zstd.get_frame_parameters(b'')

        with self.assertRaisesRegexp(zstd.ZstdError, 'not enough data for frame'):
            zstd.get_frame_parameters(zstd.FRAME_HEADER)

    def test_invalid_frame(self):
        with self.assertRaisesRegexp(zstd.ZstdError, 'Unknown frame descriptor'):
            zstd.get_frame_parameters(b'foobarbaz')

    def test_attributes(self):
        params = zstd.get_frame_parameters(zstd.FRAME_HEADER + b'\x00\x00')
        self.assertEqual(params.content_size, 0)
        self.assertEqual(params.window_size, 1024)
        self.assertEqual(params.dict_id, 0)
        self.assertFalse(params.has_checksum)

        # Lowest 2 bits indicate a dictionary and length. Here, the dict id is 1 byte.
        params = zstd.get_frame_parameters(zstd.FRAME_HEADER + b'\x01\x00\xff')
        self.assertEqual(params.content_size, 0)
        self.assertEqual(params.window_size, 1024)
        self.assertEqual(params.dict_id, 255)
        self.assertFalse(params.has_checksum)

        # Lowest 3rd bit indicates if checksum is present.
        params = zstd.get_frame_parameters(zstd.FRAME_HEADER + b'\x04\x00')
        self.assertEqual(params.content_size, 0)
        self.assertEqual(params.window_size, 1024)
        self.assertEqual(params.dict_id, 0)
        self.assertTrue(params.has_checksum)

        # Upper 2 bits indicate content size.
        params = zstd.get_frame_parameters(zstd.FRAME_HEADER + b'\x40\x00\xff\x00')
        self.assertEqual(params.content_size, 511)
        self.assertEqual(params.window_size, 1024)
        self.assertEqual(params.dict_id, 0)
        self.assertFalse(params.has_checksum)

        # Window descriptor is 2nd byte after frame header.
        params = zstd.get_frame_parameters(zstd.FRAME_HEADER + b'\x00\x40')
        self.assertEqual(params.content_size, 0)
        self.assertEqual(params.window_size, 262144)
        self.assertEqual(params.dict_id, 0)
        self.assertFalse(params.has_checksum)

        # Set multiple things.
        params = zstd.get_frame_parameters(zstd.FRAME_HEADER + b'\x45\x40\x0f\x10\x00')
        self.assertEqual(params.content_size, 272)
        self.assertEqual(params.window_size, 262144)
        self.assertEqual(params.dict_id, 15)
        self.assertTrue(params.has_checksum)

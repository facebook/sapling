import io

try:
    import unittest2 as unittest
except ImportError:
    import unittest

try:
    import hypothesis
    import hypothesis.strategies as strategies
except ImportError:
    raise unittest.SkipTest('hypothesis not available')

import zstd


compression_levels = strategies.integers(min_value=1, max_value=22)


class TestRoundTrip(unittest.TestCase):
    @hypothesis.given(strategies.binary(), compression_levels)
    def test_compress_write_to(self, data, level):
        """Random data from compress() roundtrips via write_to."""
        cctx = zstd.ZstdCompressor(level=level)
        compressed = cctx.compress(data)

        buffer = io.BytesIO()
        dctx = zstd.ZstdDecompressor()
        with dctx.write_to(buffer) as decompressor:
            decompressor.write(compressed)

        self.assertEqual(buffer.getvalue(), data)

    @hypothesis.given(strategies.binary(), compression_levels)
    def test_compressor_write_to_decompressor_write_to(self, data, level):
        """Random data from compressor write_to roundtrips via write_to."""
        compress_buffer = io.BytesIO()
        decompressed_buffer = io.BytesIO()

        cctx = zstd.ZstdCompressor(level=level)
        with cctx.write_to(compress_buffer) as compressor:
            compressor.write(data)

        dctx = zstd.ZstdDecompressor()
        with dctx.write_to(decompressed_buffer) as decompressor:
            decompressor.write(compress_buffer.getvalue())

        self.assertEqual(decompressed_buffer.getvalue(), data)

    @hypothesis.given(strategies.binary(average_size=1048576))
    @hypothesis.settings(perform_health_check=False)
    def test_compressor_write_to_decompressor_write_to_larger(self, data):
        compress_buffer = io.BytesIO()
        decompressed_buffer = io.BytesIO()

        cctx = zstd.ZstdCompressor(level=5)
        with cctx.write_to(compress_buffer) as compressor:
            compressor.write(data)

        dctx = zstd.ZstdDecompressor()
        with dctx.write_to(decompressed_buffer) as decompressor:
            decompressor.write(compress_buffer.getvalue())

        self.assertEqual(decompressed_buffer.getvalue(), data)

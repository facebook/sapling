import io

try:
    import unittest2 as unittest
except ImportError:
    import unittest

import zstd

try:
    import zstd_cffi
except ImportError:
    raise unittest.SkipTest('cffi version of zstd not available')


class TestCFFIWriteToToCDecompressor(unittest.TestCase):
    def test_simple(self):
        orig = io.BytesIO()
        orig.write(b'foo')
        orig.write(b'bar')
        orig.write(b'foobar' * 16384)

        dest = io.BytesIO()
        cctx = zstd_cffi.ZstdCompressor()
        with cctx.write_to(dest) as compressor:
            compressor.write(orig.getvalue())

        uncompressed = io.BytesIO()
        dctx = zstd.ZstdDecompressor()
        with dctx.write_to(uncompressed) as decompressor:
            decompressor.write(dest.getvalue())

        self.assertEqual(uncompressed.getvalue(), orig.getvalue())



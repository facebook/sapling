try:
    import unittest2 as unittest
except ImportError:
    import unittest

import zstd

from . common import (
    make_cffi,
)


@make_cffi
class TestSizes(unittest.TestCase):
    def test_decompression_size(self):
        size = zstd.estimate_decompression_context_size()
        self.assertGreater(size, 100000)

    def test_compression_size(self):
        params = zstd.get_compression_parameters(3)
        size = zstd.estimate_compression_context_size(params)
        self.assertGreater(size, 100000)

import sys

try:
    import unittest2 as unittest
except ImportError:
    import unittest

import zstd


if sys.version_info[0] >= 3:
    int_type = int
else:
    int_type = long


class TestTrainDictionary(unittest.TestCase):
    def test_no_args(self):
        with self.assertRaises(TypeError):
            zstd.train_dictionary()

    def test_bad_args(self):
        with self.assertRaises(TypeError):
            zstd.train_dictionary(8192, u'foo')

        with self.assertRaises(ValueError):
            zstd.train_dictionary(8192, [u'foo'])

    def test_basic(self):
        samples = []
        for i in range(128):
            samples.append(b'foo' * 64)
            samples.append(b'bar' * 64)
            samples.append(b'foobar' * 64)
            samples.append(b'baz' * 64)
            samples.append(b'foobaz' * 64)
            samples.append(b'bazfoo' * 64)

        d = zstd.train_dictionary(8192, samples)
        self.assertLessEqual(len(d), 8192)

        dict_id = d.dict_id()
        self.assertIsInstance(dict_id, int_type)

        data = d.as_bytes()
        self.assertEqual(data[0:4], b'\x37\xa4\x30\xec')

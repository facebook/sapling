from mercurial.dicthelpers import diff, join
import unittest
import silenttestrunner

class testdicthelpers(unittest.TestCase):
    def test_dicthelpers(self):
        # empty dicts
        self.assertEqual(diff({}, {}), {})
        self.assertEqual(join({}, {}), {})

        d1 = {}
        d1['a'] = 'foo'
        d1['b'] = 'bar'
        d1['c'] = 'baz'

        # same identity
        self.assertEqual(diff(d1, d1), {})
        self.assertEqual(join(d1, d1), {'a': ('foo', 'foo'),
                                        'b': ('bar', 'bar'),
                                        'c': ('baz', 'baz')})

        # vs empty
        self.assertEqual(diff(d1, {}), {'a': ('foo', None),
                                        'b': ('bar', None),
                                        'c': ('baz', None)})
        self.assertEqual(diff(d1, {}), {'a': ('foo', None),
                                        'b': ('bar', None),
                                        'c': ('baz', None)})

        d2 = {}
        d2['a'] = 'foo2'
        d2['b'] = 'bar'
        d2['d'] = 'quux'

        self.assertEqual(diff(d1, d2), {'a': ('foo', 'foo2'),
                                        'c': ('baz', None),
                                        'd': (None, 'quux')})
        self.assertEqual(join(d1, d2), {'a': ('foo', 'foo2'),
                                        'b': ('bar', 'bar'),
                                        'c': ('baz', None),
                                        'd': (None, 'quux')})

        # with default argument
        self.assertEqual(diff(d1, d2, 123), {'a': ('foo', 'foo2'),
                                             'c': ('baz', 123),
                                             'd': (123, 'quux')})
        self.assertEqual(join(d1, d2, 456), {'a': ('foo', 'foo2'),
                                             'b': ('bar', 'bar'),
                                             'c': ('baz', 456),
                                             'd': (456, 'quux')})

        # check that we compare against default
        self.assertEqual(diff(d1, d2, 'baz'), {'a': ('foo', 'foo2'),
                                               'd': ('baz', 'quux')})
        self.assertEqual(diff(d1, d2, 'quux'), {'a': ('foo', 'foo2'),
                                                'c': ('baz', 'quux')})

if __name__ == '__main__':
    silenttestrunner.main(__name__)

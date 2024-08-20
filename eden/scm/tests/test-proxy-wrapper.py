from __future__ import absolute_import

import unittest

import silenttestrunner
from sapling import util


class base:
    def foo(self):
        return self.bar()

    def bar(self):
        return "base"

    def qux(self):
        return "base"


class inner(base):
    def __init__(self):
        self.inner_attr = 456

    def bar(self):
        return "inner"

    def qux(self):
        return "inner"

    def inner_only(self):
        return "inner"


class wrapper(util.proxy_wrapper, base):
    def __init__(self, inner):
        super().__init__(inner, wrapper_attr=123)

    def bar(self):
        return "wrapper"

    def wrapper_only(self):
        return "wrapper " + self.inner.inner_only()


class ProxyWrapper(unittest.TestCase):
    def testWrap(self):
        w = wrapper(inner())

        self.assertEqual(w.wrapper_attr, 123)
        self.assertEqual(w.inner_attr, 456)

        w.new_attr = "a"
        self.assertEqual(w.new_attr, "a")
        self.assertEqual(w.inner.new_attr, "a")

        w.wrapper_attr = "b"
        self.assertEqual(w.wrapper_attr, "b")

        w.inner_attr = "c"
        self.assertEqual(w.inner_attr, "c")
        self.assertEqual(w.inner.inner_attr, "c")

        # Wrapper and inner override "bar" - use wrapper's
        self.assertEqual(w.bar(), "wrapper")

        # Neither override "foo" - use wrapper's
        self.assertEqual(w.foo(), "wrapper")

        # Inner overrides "qux" - use inner's
        self.assertEqual(w.qux(), "inner")

        # Only exists on inner
        self.assertEqual(w.inner_only(), "inner")

        # Only exists in wrapper
        self.assertEqual(w.wrapper_only(), "wrapper inner")


if __name__ == "__main__":
    silenttestrunner.main(__name__)

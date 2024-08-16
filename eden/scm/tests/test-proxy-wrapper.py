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


class wrapped(base):
    def __init__(self):
        self.wrapped_attr = 456

    def bar(self):
        return "wrapped"

    def qux(self):
        return "wrapped"

    def wrapped_only(self):
        return "wrapped"


class wrapper(util.proxy_wrapper, base):
    def __init__(self, wrapped):
        super().__init__(wrapped, wrapper_attr=123)

    def bar(self):
        return "wrapper"

    def wrapper_only(self):
        return "wrapper " + self.inner.wrapped_only()


class ProxyWrapper(unittest.TestCase):
    def testWrap(self):
        inner = wrapped()
        w = wrapper(inner)

        self.assertEqual(w.wrapper_attr, 123)
        self.assertEqual(w.wrapped_attr, 456)

        w.new_attr = "a"
        self.assertEqual(w.new_attr, "a")
        self.assertEqual(w.inner.new_attr, "a")

        w.wrapper_attr = "b"
        self.assertEqual(w.wrapper_attr, "b")

        w.wrapped_attr = "c"
        self.assertEqual(w.wrapped_attr, "c")
        self.assertEqual(w.inner.wrapped_attr, "c")

        # Wrapper and wrapped override "bar" - use wrapper's
        self.assertEqual(w.bar(), "wrapper")

        # Neither override "foo" - use wrapper's
        self.assertEqual(w.foo(), "wrapper")

        # Wrapped overrides "qux" - use wrapped's
        self.assertEqual(w.qux(), "wrapped")

        # Only exists on wrapped
        self.assertEqual(w.wrapped_only(), "wrapped")

        # Only exists in wrapper
        self.assertEqual(w.wrapper_only(), "wrapper wrapped")


if __name__ == "__main__":
    silenttestrunner.main(__name__)

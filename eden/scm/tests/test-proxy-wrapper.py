from __future__ import absolute_import

import unittest

import silenttestrunner
from sapling import util


def unbound():
    return "unbound"


class base:
    _base_unbound = unbound
    _base_class_attr = 123

    def base_only(self):
        return self.both_override()

    def both_override(self):
        return "base"

    def wrapper_overrides(self):
        return "base"

    def inner_overrides(self):
        return "base"

    @util.propertycache
    def base_property(self):
        return "base"

    def middle_overrides(self):
        return "base"

    @util.propertycache
    def neither_overrides(self):
        return self.both_override


class middle(base):
    def middle_overrides(self):
        return "middle"


class inner(middle):
    def __init__(self):
        self.inner_attr = 456

    def both_override(self):
        return "inner"

    def inner_overrides(self):
        return "inner"

    def inner_only(self):
        return "inner"

    @util.propertycache
    def inner_property(self):
        return "inner"

    def middle_overrides(self):
        return super().middle_overrides()


class wrapper(util.proxy_wrapper, base):
    def __init__(self, inner):
        super().__init__(inner, wrapper_attr=123, unbound=unbound)

    def both_override(self):
        return "wrapper"

    def wrapper_overrides(self):
        return "wrapper"

    def wrapper_only(self):
        return "wrapper " + self.inner.inner_only()

    @util.propertycache
    def wrapper_property(self):
        return "wrapper"

    @util.propertycache
    def dont_call_me(self):
        raise Exception("property called eagerly!")

    def __call__(self):
        return "wrapper"


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

        # Wrapper and inner override "both_override" - use wrapper's
        self.assertEqual(w.both_override(), "wrapper")

        # Neither override "base_only" - use wrapper's
        self.assertEqual(w.base_only(), "wrapper")

        # Wrapper overrides - use wrapper's
        self.assertEqual(w.wrapper_overrides(), "wrapper")

        # Inner overrides "inner_overrides" - use inner's
        self.assertEqual(w.inner_overrides(), "inner")

        # Only exists on inner
        self.assertEqual(w.inner_only(), "inner")

        # Only exists in wrapper
        self.assertEqual(w.wrapper_only(), "wrapper inner")

        # Base method that calls a method overidden in wrapper
        self.assertEqual(w.neither_overrides(), "wrapper")

        # Inner method that calls a method overidden in wrapper
        self.assertEqual(w.middle_overrides(), "middle")

        # Delegate class attributes to inner object.
        # XXX FIXME - shouldn't crash
        self.assertEqual(w._base_class_attr, 123)

        w._base_class_attr = 456
        self.assertEqual(w._base_class_attr, 456)
        self.assertEqual(w.inner._base_class_attr, 456)

        w.inner._base_class_attr = 123
        self.assertEqual(w._base_class_attr, 123)
        self.assertEqual(w.inner._base_class_attr, 123)

        self.assertEqual(w.wrapper_property, "wrapper")
        self.assertEqual(w.base_property, "base")
        self.assertEqual(w.inner_property, "inner")

        self.assertEqual(w(), "wrapper")

        self.assertEqual(w.unbound(), "unbound")
        self.assertEqual(w.__class__._base_unbound(), "unbound")


if __name__ == "__main__":
    silenttestrunner.main(__name__)

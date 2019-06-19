#!/usr/bin/env python3
#
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import unittest

from .. import util


class UtilTest(unittest.TestCase):
    def test_is_valid_sha1(self):
        def is_valid(sha1: str):
            return util.is_valid_sha1(sha1)

        self.assertTrue(is_valid("0123456789abcabcabcd0123456789abcabcabcd"))
        self.assertTrue(is_valid("0" * 40))

        self.assertFalse(is_valid("0123456789abcabcabcd0123456789abcabcabc"))
        self.assertFalse(is_valid("z123456789abcabcabcd0123456789abcabcabcd"))
        self.assertFalse(is_valid(None))
        self.assertFalse(is_valid(""))
        self.assertFalse(is_valid("abc"))
        self.assertFalse(is_valid("z" * 40))

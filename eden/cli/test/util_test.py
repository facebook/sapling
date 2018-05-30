#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

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

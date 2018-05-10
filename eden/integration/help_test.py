#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import unittest

from .lib import edenclient


class HelpTest(unittest.TestCase):
    """
    This test verifies the Eden CLI can at least load its Python code.
    It can be removed when the remaining integration tests are enabled
    on sandcastle.
    """

    def test_eden_cli_help_returns_without_error(self) -> None:
        with edenclient.EdenFS() as client:
            return_code = client.run_unchecked("help")
            self.assertEqual(0, return_code)

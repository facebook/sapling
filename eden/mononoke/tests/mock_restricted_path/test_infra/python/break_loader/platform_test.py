#!/usr/bin/env python3
# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

# pyre-strict
import os
import sys
import unittest

from eden.mononoke.tests.mock_restricted_path.test_infra.python.break_loader.platform import (
    add,
)


class BreakLoaderPlaygroundTest(unittest.TestCase):
    """
    This is a special test case. It's important that it loads a Python module
    where the end of the module name matches that of a builtin Python module.
    It acts as regression test for a custom module loader bug.

    The bug only showed up in par_style=fastzip.
    """

    def test_playground(self) -> None:
        print("playground stdout")
        print("playground stderr", file=sys.stderr)
        if os.environ.get("TPX_PLAYGROUND_FAIL") is not None:
            self.assertEqual(42, 41)
        elif os.environ.get("TPX_PLAYGROUND_FATAL") is not None:
            os._exit(1)
        else:
            self.assertEqual(add(21, 21), 42)

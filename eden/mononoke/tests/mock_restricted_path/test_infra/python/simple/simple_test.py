#!/usr/bin/env python3
# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

# pyre-ignore-all-errors
import os
import sys
import time
import unittest

try:
    from eden.mononoke.tests.mock_restricted_path.test_infra.python.simple.more import (
        inverse,
    )
    from eden.mononoke.tests.mock_restricted_path.test_infra.python.simple.simple import (
        add,
    )
except ImportError:
    from base_module_mapped.more import inverse

    # Could be base module mapped. Try that instead.
    from base_module_mapped.simple import add


class SimplePlaygroundTest(unittest.TestCase):
    def test_python_source_map_exists(self) -> None:
        source_mapping_file_path = os.environ.get("PYTHON_SOURCE_MAP")
        self.assertIsNotNone(source_mapping_file_path)

    def test_playground(self) -> None:
        print("playground stdout")
        print("playground stderr", file=sys.stderr)
        if os.environ.get("TPX_PLAYGROUND_FAIL") is not None:
            self.assertEqual(42, 41)
        elif os.environ.get("TPX_PLAYGROUND_FATAL") is not None:
            os._exit(1)
        elif os.environ.get("TPX_PLAYGROUND_SLEEP") is not None:
            time.sleep(int(os.environ.get("TPX_PLAYGROUND_SLEEP")))

        self.assertEqual(add(21, 21), 42)
        self.assertEqual(inverse(42), -42)

        # skip at the end, otherwise coverage tests will fail and we don't want
        # that to happen
        if os.environ.get("TPX_PLAYGROUND_SKIP") is not None:
            self.skipTest("Skip test")

    def test_playground2(self) -> None:
        self.assertEqual(add(21, 21), 42)

    def test_playground_should_have_test_env_set(self) -> None:
        self.assertIsNotNone(os.environ.get("TPX_IS_TEST_EXECUTION"))

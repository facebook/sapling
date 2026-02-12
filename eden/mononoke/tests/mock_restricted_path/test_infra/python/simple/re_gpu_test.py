#!/usr/bin/env python3
# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

# pyre-strict
import os
import unittest


class REGPUTest(unittest.TestCase):
    def test_playground(self) -> None:
        self.assertEqual(2 + 2, 4)

    # verify that TPX_IS_TEST_EXECUTION is also set in remote execution environments
    def test_playground_should_have_test_env_set(self) -> None:
        self.assertIsNotNone(os.environ.get("TPX_IS_TEST_EXECUTION"))

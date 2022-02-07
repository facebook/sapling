#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib
import unittest
import warnings
from pathlib import Path
from typing import Optional, Union, cast

from . import environment_variable as env_module
from .temporary_directory import TempFileManager


try:
    from unittest import IsolatedAsyncioTestCase
except ImportError:
    from .async_case_backport import IsolatedAsyncioTestCase


@contextlib.contextmanager
def no_warnings(self: unittest.TestCase):
    with warnings.catch_warnings(record=True) as wlist:
        yield

    if wlist:
        msgs = [
            warnings.formatwarning(
                cast(str, w.message), w.category, w.filename, w.lineno, w.line
            )
            for w in wlist
        ]
        self.fail("Warnings detected during test:\n" + "".join(msgs))


class EdenTestCaseBase(IsolatedAsyncioTestCase):
    """Base class for many EdenFS test cases.

    This class provides a number of convenience functions.
    """

    exit_stack: contextlib.ExitStack
    temp_mgr: TempFileManager

    def setUp(self) -> None:
        super().setUp()
        self.exit_stack = contextlib.ExitStack()
        self.addCleanup(self.exit_stack.close)
        self.temp_mgr = self.exit_stack.enter_context(
            TempFileManager(self._get_tmp_prefix())
        )

    def _callSetUp(self):
        with no_warnings(self):
            return super()._callSetUp()

    def _callTearDown(self):
        with no_warnings(self):
            return super()._callTearDown()

    def _callTestMethod(self, testMethod):
        with no_warnings(self):
            return super()._callTestMethod(testMethod)

    def _get_tmp_prefix(self) -> str:
        """Get a prefix to use for the test's temporary directory name."""
        # Attempt to include a potion of the test name in the temporary directory
        # prefix, but limit it to 20 characters.  If the path is too long EdenFS will
        # fail to start since its Unix socket path won't fit in sockaddr_un, which has a
        # 108 byte maximum path length.
        method_name = self._testMethodName
        for strip_prefix in ("test_", "test"):
            if method_name.startswith(strip_prefix):
                method_name = method_name[len(strip_prefix) :]
                break
        return f"eden_test.{method_name[:10]}."

    def setenv(self, name: str, value: Optional[str]) -> None:
        self.exit_stack.enter_context(env_module.setenv_scope(name, value))

    def unsetenv(self, name: str) -> None:
        self.exit_stack.enter_context(env_module.unsetenv_scope(name))

    @property
    def tmp_dir(self) -> Path:
        return self.temp_mgr.top_level_tmp_dir()

    def make_temp_dir(self, prefix: Optional[str] = None) -> Path:
        """Make a directory with a uniquely-generated name under the top-level test-case
        subdirectory.
        """
        return self.temp_mgr.make_temp_dir(prefix=prefix)

    def make_test_dir(self, name: Union[Path, str], parents: bool = True) -> Path:
        """Make a directory with a specific name under the top-level test-case
        subdirectory.
        """
        dir_path = self.tmp_dir / name
        dir_path.mkdir(parents=parents)
        return dir_path

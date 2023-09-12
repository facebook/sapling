#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib
import sys
import unittest
import warnings
from pathlib import Path
from typing import cast, Optional, Union

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

        # macOS by default has us using a temporary directory with a long path
        # under /private/var/folders.  This can cause tests that use unix-domain
        # sockets to fail, so here we set an environment variable to override
        # our temporary directory to /tmp.
        if sys.platform == "darwin":
            self.setenv("TMPDIR", "/tmp")

        self.temp_mgr = self.exit_stack.enter_context(TempFileManager())

    def _callSetUp(self):
        with no_warnings(self):
            return super()._callSetUp()

    def _callTearDown(self):
        with no_warnings(self):
            return super()._callTearDown()

    def _callTestMethod(self, testMethod):
        with no_warnings(self):
            return super()._callTestMethod(testMethod)

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

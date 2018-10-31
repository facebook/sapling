#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import abc
import contextlib
import os
import pathlib
import tempfile
import typing

from .util import cleanup_tmp_dir


# TODO(strager): Merge create_tmp_dir with
# TemporaryDirectoryMixin.make_temporary_directory.


@contextlib.contextmanager
def create_tmp_dir() -> typing.Iterator[pathlib.Path]:
    """A helper class to manage temporary directories for snapshots.

    This is similar to the standard tempdir.TemporaryDirectory code,
    but does a better job of cleaning up the directory if some of its contents are
    read-only.
    """
    tmpdir = pathlib.Path(tempfile.mkdtemp(prefix="eden_data."))
    try:
        yield tmpdir
    finally:
        cleanup_tmp_dir(tmpdir)


class TemporaryDirectoryMixin(metaclass=abc.ABCMeta):
    def make_temporary_directory(self) -> str:
        def clean_up(path_str: str) -> None:
            if os.environ.get("EDEN_TEST_NO_CLEANUP"):
                print("Leaving behind eden test directory %r" % path_str)
            else:
                cleanup_tmp_dir(pathlib.Path(path_str))

        path_str = tempfile.mkdtemp(prefix="eden_test.")
        self.addCleanup(lambda: clean_up(path_str))
        return path_str

    def addCleanup(
        self,
        function: typing.Callable[..., typing.Any],
        *args: typing.Any,
        **kwargs: typing.Any
    ) -> None:
        raise NotImplementedError()

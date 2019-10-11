#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib
import pathlib
import tempfile
import typing

from eden.test_support.temporary_directory import cleanup_tmp_dir


# TODO(strager): Merge create_tmp_dir with
# eden.test_support.temporary_directory.TemporaryDirectoryMixin.make_temporary_directory.


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

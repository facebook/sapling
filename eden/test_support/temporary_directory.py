#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import abc
import logging
import os
import pathlib
import shutil
import tempfile
import types
import typing
from pathlib import Path
from typing import Any, Callable, Tuple, Type, Union


def cleanup_tmp_dir(tmp_dir: Path) -> None:
    """Clean up a temporary directory.

    This is similar to shutil.rmtree() but also handles removing read-only files and
    directories.  This function changes the permissions on files and directories if this
    is necessary to remove them.

    This is necessary for removing Eden checkout directories since "eden clone" makes
    the original mount point directory read-only.
    """
    # If we encounter an EPERM or EACCESS error removing a file try making its parent
    # directory writable and then retry the removal.
    def _remove_readonly(
        func: Callable[[Union[os.PathLike, str]], Any],
        path: Union[os.PathLike, str],
        exc_info: Tuple[Type, BaseException, types.TracebackType],
    ) -> None:
        _ex_type, ex, _traceback = exc_info
        # pyre-fixme[29]: `Union[Callable[[object], bool], Callable[[object], bool],
        #  Callable[[object], bool], Callable[[object], bool]]` is not a function.
        if path == tmp_dir:
            logging.warning(
                f"failed to remove temporary test directory {tmp_dir}: {ex}"
            )
            return
        if not isinstance(ex, PermissionError):
            logging.warning(f"error removing file in temporary directory {path}: {ex}")
            return

        try:
            # pyre-fixme[6]: Expected `_PathLike[AnyStr]` for 1st param but got
            #  `Union[_PathLike[Any], str]`.
            parent_dir = os.path.dirname(path)
            os.chmod(parent_dir, 0o755)
            # func() is the function that failed.
            # This is usually os.unlink() or os.rmdir().
            func(path)
        except OSError as ex:
            logging.warning(f"error removing file in temporary directory {path}: {ex}")
            return

    shutil.rmtree(tmp_dir, onerror=_remove_readonly)


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
        **kwargs: typing.Any,
    ) -> None:
        raise NotImplementedError()

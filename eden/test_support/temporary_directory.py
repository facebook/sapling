#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-strict

import abc
import logging
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import types
import typing
from pathlib import Path
from typing import (
    Any,
    BinaryIO,
    Callable,
    Generic,
    Optional,
    TextIO,
    Tuple,
    Type,
    TypeVar,
    Union,
)


def _unmount_leaked_mounts(tmp_dir: Path) -> None:
    """Lazily unmount any mounts left behind under tmp_dir.

    A mount can be left behind when a test kills the EdenFS daemon and its
    privhelper without unmounting (e.g. tests exercising recovery from an
    unclean shutdown), or when unmounting fails during normal daemon cleanup.
    shutil.rmtree() cannot remove a mount point, and a leaked mount whose
    server is gone stalls every process that scans the mount table (df, and
    through it chef) until the client request times out.
    """
    if sys.platform != "linux":
        return
    prefix = str(tmp_dir) + "/"
    try:
        with open("/proc/mounts") as f:
            # The mount point is the second space-separated field, with
            # special characters octal-escaped (e.g. "\040" for space).
            mount_points = [
                line.split(" ")[1].encode().decode("unicode_escape") for line in f
            ]
    except OSError as ex:
        logging.warning(f"error listing mounts under {tmp_dir}: {ex}")
        return

    leaked = sorted((m for m in mount_points if m.startswith(prefix)), reverse=True)
    for mount_point in leaked:
        # Sorted in reverse so that nested mounts are unmounted before their
        # parents. Lazy unmount (MNT_DETACH) always detaches the mount from
        # the mount table, even if the mount is wedged or still in use.
        cmd = ["umount", "-l", mount_point]
        if os.geteuid() != 0:
            cmd = ["sudo", "-n"] + cmd
        result = subprocess.run(
            cmd, capture_output=True, encoding="utf-8", errors="replace"
        )
        if result.returncode == 0:
            logging.warning(f"unmounted mount leaked by test: {mount_point}")
        else:
            logging.warning(
                f"failed to unmount mount leaked by test: {mount_point}: "
                f"{result.stderr.strip()}"
            )


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
        # pyre-fixme[24]: Generic type `os.PathLike` expects 1 type parameter.
        func: Callable[[Union[os.PathLike, str]], Any],  # pyre-fixme[2]
        # pyre-fixme[24]: Generic type `os.PathLike` expects 1 type parameter.
        path: Union[os.PathLike, str],
        exc_info: Tuple[Type[BaseException], BaseException, types.TracebackType],
    ) -> None:
        _ex_type, ex, _traceback = exc_info
        if path == tmp_dir:
            logging.warning(
                f"failed to remove temporary test directory {tmp_dir}: {ex}"
            )
            return
        if not isinstance(ex, PermissionError):
            logging.warning(f"error removing file in temporary directory {path}: {ex}")
            return

        try:
            # func() is the function that failed.
            # This is usually os.unlink() or os.rmdir(). I have started to see
            # open fail as well. We don't have all the right arguments to retry
            # the call, so we just have to deal with the failure. If you are
            # debugging failures here, this is probably not the root cause of
            # the problem, but a error cleaning up a broken test.
            if func not in (os.unlink, os.rmdir):
                raise ex

            # pyre-fixme[6]: For 1st argument expected `PathLike[Variable[AnyStr <:
            #  [str, bytes]]]` but got `Union[PathLike[typing.Any], str]`.
            parent_dir = os.path.dirname(path)
            os.chmod(parent_dir, 0o755)
            func(path)
        except OSError as ex:
            logging.warning(f"error removing file in temporary directory {path}: {ex}")
            return

    shutil.rmtree(tmp_dir, onerror=_remove_readonly)
    if tmp_dir.exists():
        # rmtree cannot remove mount points; unmount anything left mounted
        # under tmp_dir and try once more.
        _unmount_leaked_mounts(tmp_dir)
        shutil.rmtree(tmp_dir, onerror=_remove_readonly)


class TempFileManager:
    """TempFileManager exists for managing a set of temporary files and directories.

    It creates all temporary files and directories in a single top-level directory,
    which can later be cleaned up in one pass.

    This helps make it a little easier to track temporary test artifacts while
    debugging, and helps make it easier to identify when tests have failed to clean up
    their temporary files.

    This is also necessary on Windows because the standard tempfile.NamedTemporaryFile
    class unfortunately does not work well there: the temporary files it creates cannot
    be opened by other processes.
    """

    _temp_dir: Optional[Path] = None
    _prefix: str = "eden_test."

    def __enter__(self) -> "TempFileManager":
        return self

    def __exit__(
        self,
        exc_type: Optional[Type[BaseException]],
        exc_value: Optional[BaseException],
        tb: Optional[types.TracebackType],
    ) -> None:
        self.cleanup(exc_type is not None)

    def cleanup(self, failure: bool = False) -> None:
        temp_dir = self._temp_dir
        if temp_dir is None:
            return

        cleanup_mode = os.environ.get("EDEN_TEST_CLEANUP", "always").lower()
        if cleanup_mode in ("0", "no", "false") or (
            failure and cleanup_mode == "success-only"
        ):
            print(f"Leaving behind eden test directory {temp_dir}")
        else:
            cleanup_tmp_dir(temp_dir)
        self._temp_dir = None

    def make_temp_dir(self, prefix: Optional[str] = None) -> Path:
        top_level = self.top_level_tmp_dir()
        path_str = tempfile.mkdtemp(prefix=prefix, dir=str(top_level))
        return Path(path_str)

    def make_temp_file(
        self, prefix: Optional[str] = None, mode: str = "r+"
    ) -> "TemporaryTextFile":
        top_level = self.top_level_tmp_dir()
        fd, path_str = tempfile.mkstemp(prefix=prefix, dir=str(top_level))
        file_obj = os.fdopen(fd, mode, encoding="utf-8")
        # pyre-fixme[6]: Expected `TextIO` for 1st param but got `IO[typing.Any]`.
        return TemporaryTextFile(file_obj, Path(path_str))

    def make_temp_binary(
        self, prefix: Optional[str] = None, mode: str = "rb+"
    ) -> "TemporaryBinaryFile":
        top_level = self.top_level_tmp_dir()
        fd, path_str = tempfile.mkstemp(prefix=prefix, dir=str(top_level))
        file_obj = os.fdopen(fd, mode)
        # pyre-fixme[6]: Expected `BinaryIO` for 1st param but got `IO[typing.Any]`.
        return TemporaryBinaryFile(file_obj, Path(path_str))

    def top_level_tmp_dir(self) -> Path:
        top = self._temp_dir
        if top is None:
            top = Path(tempfile.mkdtemp(prefix=self._prefix)).resolve()
            self._temp_dir = top

        return top

    def set_tmp_prefix(self, prefix: str) -> None:
        if self._temp_dir is not None:
            logging.warning(
                f"cannot update temporary directory prefix to {prefix}: "
                f"temporary directory {self._temp_dir} was already created"
            )
            return
        self._prefix = prefix


IOType = TypeVar("IOType", TextIO, BinaryIO)
T = TypeVar("T")


class TemporaryFileBase(Generic[IOType]):
    """This class is largely equivalent to tempfile.NamedTemporaryFile,
    but it also works on Windows.  (The standard library NamedTemporaryFile class
    creates files that cannot be opened by other processes.)

    We don't have any logic for closing the file here since the entire containing
    directory will eventually be removed by TempFileManager.
    """

    file: IOType
    name: str
    path: Path

    def __init__(self, file: IOType, path: Path) -> None:
        self.file = file
        self.path = path
        self.name = str(path)

    def __getattr__(self, name: str) -> Any:  # pyre-fixme[3]
        if name in ("name", "path"):
            return self.__dict__[name]
        else:
            file = self.__dict__["file"]
            value = getattr(file, name)
            setattr(self, name, value)
            return value

    def __enter__(self: T) -> T:
        return self

    def __exit__(
        self,
        exc_type: Optional[Type[BaseException]],
        exc_value: Optional[BaseException],
        tb: Optional[types.TracebackType],
    ) -> None:
        self.file.close()


class TemporaryTextFile(TemporaryFileBase[TextIO]):
    pass


class TemporaryBinaryFile(TemporaryFileBase[BinaryIO]):
    pass


class TemporaryDirectoryMixin(metaclass=abc.ABCMeta):
    temp_file_manager: TempFileManager = TempFileManager()
    _temp_cleanup_added: bool = False

    def make_temporary_directory(self, prefix: Optional[str] = None) -> str:
        self._ensure_temp_cleanup()
        return str(self.temp_file_manager.make_temp_dir(prefix=prefix))

    def _ensure_temp_cleanup(self) -> None:
        if not self._temp_cleanup_added:
            self.addCleanup(self.temp_file_manager.cleanup)
            self._temp_cleanup_added = True

    def addCleanup(
        self, function: Callable[[], None], *args: Any, **kwargs: Any
    ) -> None:
        raise NotImplementedError()

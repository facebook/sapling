#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import logging
import os
import shutil
import types
from pathlib import Path
from typing import Any, Callable, List, Optional, Tuple, Type


def gen_tree(
    path: str,
    fanouts: List[int],
    leaf_function: Callable[[str], None],
    internal_function: Optional[Callable[[str], None]] = None,
) -> None:
    """
    Helper function for recursively building a large branching directory
    tree.

    path is the leading path prefix to put before all directory names.

    fanouts is an array of integers specifying the directory fan-out
    dimensions.  One layer of directories will be created for each element
    in this array.  e.g., [3, 4] would create 3 subdirectories inside the
    top-level directory, and 4 subdirectories in each of those 3
    directories.

    Calls leaf_function on all leaf directories.
    Calls internal_function on all internal (non-leaf) directories.
    """
    for n in range(fanouts[0]):
        subdir = os.path.join(path, "dir{:02}".format(n + 1))
        sub_fanouts = fanouts[1:]
        if sub_fanouts:
            if internal_function is not None:
                internal_function(subdir)
            gen_tree(subdir, fanouts[1:], leaf_function, internal_function)
        else:
            leaf_function(subdir)


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
        func: Callable[[str], Any],
        path: str,
        exc_info: Tuple[Type, BaseException, types.TracebackType],
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
            parent_dir = os.path.dirname(path)
            os.chmod(parent_dir, 0o755)
            # func() is the function that failed.
            # This is usually os.unlink() or os.rmdir().
            func(path)
        except OSError as ex:
            logging.warning(f"error removing file in temporary directory {path}: {ex}")
            return

    shutil.rmtree(tmp_dir, onerror=_remove_readonly)

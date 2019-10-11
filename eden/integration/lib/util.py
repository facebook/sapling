#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
from typing import Callable, List, Optional


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

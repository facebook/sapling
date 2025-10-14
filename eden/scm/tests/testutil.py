# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Utility functions for unit tests."""

import os
import warnings


def fork_suppress_multithread_warning() -> int:
    """Call os.fork() suppressing the Python 3.12 multi-threaded warning."""
    with warnings.catch_warnings():
        warnings.filterwarnings(
            "ignore",
            r"This process \(pid=\d+\) is multi-threaded, use of fork\(\) may lead "
            r"to deadlocks in the child\.",
            category=DeprecationWarning,
        )
        return os.fork()

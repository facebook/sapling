# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


def path_starts_with(path, prefix):
    """Return True if 'path' is the same as 'prefix' or lives underneath it.

    Examples
    --------
    >>> path_starts_with("/var/log/nginx/error.log", "/var/log")
    True
    >>> path_starts_with("/var/logs", "/var/log")   # subtle typo
    False
    >>> path_starts_with("src/module/util.py", "src")  # relative paths fine
    True
    """
    if path == prefix:
        return True
    return path.startswith(prefix + "/")

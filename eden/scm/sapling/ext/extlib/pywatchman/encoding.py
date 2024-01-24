# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# no unicode literals
from __future__ import absolute_import, division, print_function

import sys

from . import compat


"""Module to deal with filename encoding on the local system, as returned by
Watchman."""


default_local_errors = "surrogateescape"


def get_local_encoding():
    if sys.platform == "win32":
        # Watchman always returns UTF-8 encoded strings on Windows.
        return "utf-8"
    # On the Python 3 versions we support, sys.getfilesystemencoding never
    # returns None.
    return sys.getfilesystemencoding()


def encode_local(s):
    return s.encode(get_local_encoding(), default_local_errors)


def decode_local(bs):
    return bs.decode(get_local_encoding(), default_local_errors)

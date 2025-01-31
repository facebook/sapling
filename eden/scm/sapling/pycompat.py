# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pycompat.py - portability shim for python 3
#
# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Mercurial portability shim for python 3.

This contains aliases to hide python version-specific details from the core.
"""

from __future__ import absolute_import

import abc


def identity(a):
    return a


def ensurestr(s):
    if isinstance(s, bytes):
        s = s.decode("utf-8")
    return s


ABC = abc.ABC
import collections.abc

Mapping = collections.abc.Mapping
Set = collections.abc.Set

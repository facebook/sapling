# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# no unicode literals
from __future__ import absolute_import, division, print_function

import collections.abc as abc

"""Compatibility module across Python 2 and 3."""


# This is adapted from https://bitbucket.org/gutworth/six, and used under the
# MIT license. See LICENSE for a full copyright notice.


def reraise(tp, value, tb=None):
    try:
        if value is None:
            value = tp()
        if value.__traceback__ is not tb:
            raise value.with_traceback(tb)
        raise value
    finally:
        value = None
        tb = None


UNICODE = str

# pyre-fixme[11]: Annotation `abc` is not defined as a type.
collections_abc = abc

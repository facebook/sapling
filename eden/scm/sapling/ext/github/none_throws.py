# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import Optional, TypeVar


_T = TypeVar("_T")


def none_throws(optional: Optional[_T], msg: str = "Unexpected None") -> _T:
    """unwraps Optional[T] as T in a way the type system understands."""
    assert optional is not None, msg
    return optional

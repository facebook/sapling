# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import List

def patches(text: bytes, bins: List[bytes]) -> bytes: ...
def patchedsize(orig: int, bin: bytes) -> int: ...

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import List, Tuple, Union

def blocks(a: str, b: str) -> List[Tuple[int, int, int, int]]: ...
def fixws(s: str, allws: bool) -> bytes: ...
def bdiff(a: Union[str, bytes], b: Union[str, bytes]) -> bytes: ...

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# pyre-unsafe

import sys
from typing import Optional

if sys.platform == "win32":
    from ntapi import Handle
else:
    # Dummy for pyre on Linux
    class Handle:
        pass

def open_directory_handle(s: str) -> Handle: ...
def open_file_handle(s: str, mode: str, flags: int) -> Handle: ...
def query_directory_file_ex(
    h: Handle, bufSize: int, queryFlags: int, fileName: Optional[str]
) -> list[str]: ...
def get_directory_entry_size() -> int: ...

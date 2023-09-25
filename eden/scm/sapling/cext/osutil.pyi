# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from typing import BinaryIO, List, Optional, Tuple, Union

O_CLOEXEC: int

class stat:
    st_dev: int
    st_mode: int
    st_nlink: int
    st_size: int
    st_mtime: int
    st_ctime: int

def listdir(
    path: str, stat: Optional[bool] = None, skip: Optional[str] = None
) -> Union[List[Tuple[str, int]], List[Tuple[str, int, stat]]]: ...
def posixfile(name: str, mode: str = "rb", bufsize: int = -1) -> BinaryIO: ...
def recvfds(fd: int) -> List[int]: ...
def setprocname(name: Union[str, bytes]) -> None: ...
def unblocksignal(signal: int) -> None: ...

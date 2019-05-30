# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.


from __future__ import absolute_import, division, print_function, unicode_literals

import ctypes
import os

from thrift.transport.TSocket import TSocket


class SOCKADDR_UN(ctypes.Structure):
    _fields_ = [("sun_family", ctypes.c_ushort), ("sun_path", ctypes.c_char * 108)]


class WindowsSocketException(Exception):
    def __init__(self, code):
        # type: (int) -> None
        super(WindowsSocketException, self).__init__(
            "Windows Socket Error: {}".format(code)
        )


class WindowsSocketHandle(object):
    __ws2_32 = None

    AF_UNIX = 1
    SOCK_STREAM = 1

    fd = -1  # type: int
    address = ""  # type: str

    @staticmethod
    def _ws2_32():
        if WindowsSocketHandle.__ws2_32 is None:
            WindowsSocketHandle.__ws2_32 = ctypes.windll.LoadLibrary("ws2_32.dll")
        return WindowsSocketHandle.__ws2_32

    @staticmethod
    def _checkReturnCode(retcode):
        if retcode == -1:
            errcode = WindowsSocketHandle._ws2_32().WSAGetLastError()
            raise WindowsSocketException(errcode)

    def __init__(self):
        # type: () -> None
        fd = self._ws2_32().socket(self.AF_UNIX, self.SOCK_STREAM, 0)
        self._checkReturnCode(fd)
        self.fd = fd

    def fileno(self):
        # type: () -> int
        return self.fd

    def settimeout(self, timeout):
        # type: (int) -> None
        # TODO: implement this method via `setsockopt`
        return None

    def connect(self, address):
        # type: (str) -> None
        addr = SOCKADDR_UN(sun_family=self.AF_UNIX, sun_path=address.encode("utf-8"))
        self._checkReturnCode(
            self._ws2_32().connect(self.fd, ctypes.pointer(addr), ctypes.sizeof(addr))
        )
        self.address = address

    def send(self, buff):
        # type: (bytes) -> int
        retcode = self._ws2_32().send(self.fd, buff, len(buff), 0)
        self._checkReturnCode(retcode)
        return retcode

    def recv(self, size):
        # type: (int) -> bytes
        buff = ctypes.create_string_buffer(size)
        self._checkReturnCode(self._ws2_32().recv(self.fd, buff, size, 0))
        return buff.raw

    def getpeername(self):
        # type: () -> str
        return self.address

    def getsockname(self):
        # type: () -> str
        return self.address

    def close(self):
        # type: () -> int
        return self._ws2_32().closesocket(self.fd)


class EdenTSocket(TSocket):
    @property
    def _shouldUseWinsocket(self):
        # type: () -> bool
        return os.name == "nt" and self._unix_socket

    def open(self):
        # type: () -> None
        if not self._shouldUseWinsocket:
            return super(EdenTSocket, self).open()

        handle = WindowsSocketHandle()
        self.setHandle(handle)
        try:
            handle.connect(self._unix_socket)
        except Exception:
            self.close()
            raise

# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import, division, print_function, unicode_literals

import ctypes
import os

from thrift.transport.TSocket import TSocket
from thrift.transport.TTransport import TTransportException


if os.name == "nt":
    import ctypes.wintypes


# This is a Windows only script which enables the Python Thrift client to use
# Unix Domain Socket. Most of the pyre errors in this files are because of
# missing windll on the linux system.

# WSA Error codes
WSAECONNREFUSED = 10061

# int WSAStartup(
#     WORD      wVersionRequired,
#     LPWSADATA lpWSAData
# );
WSADESCRIPTION_LEN = 256 + 1
WSASYS_STATUS_LEN = 128 + 1


class WSAData64(ctypes.Structure):
    _fields_ = [
        ("wVersion", ctypes.c_ushort),
        ("wHighVersion", ctypes.c_ushort),
        ("iMaxSockets", ctypes.c_ushort),
        ("iMaxUdpDg", ctypes.c_ushort),
        ("lpVendorInfo", ctypes.c_char_p),
        ("szDescription", ctypes.c_ushort * WSADESCRIPTION_LEN),
        ("szSystemStatus", ctypes.c_ushort * WSASYS_STATUS_LEN),
    ]


WSAStartup = ctypes.windll.ws2_32.WSAStartup
WSAStartup.argtypes = [ctypes.wintypes.WORD, ctypes.POINTER(WSAData64)]
WSAStartup.restype = ctypes.c_int


# Win32 socket API
# SOCKET WSAAPI socket(
#   int af,
#   int type,
#   int protocol
# );
socket = ctypes.windll.ws2_32.socket
socket.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_int]
socket.restype = ctypes.wintypes.HANDLE


# int connect(
#   SOCKET         s,
#   const sockaddr * name,
#   int            namelen
#   );
connect = ctypes.windll.ws2_32.connect
connect.argtypes = [ctypes.wintypes.HANDLE, ctypes.c_void_p, ctypes.c_int]
connect.restype = ctypes.c_int


# int WSAAPI send(
#   SOCKET     s,
#   const char *buf,
#   int        len,
#   int        flags
# );
send = ctypes.windll.ws2_32.send
send.argtypes = [ctypes.wintypes.HANDLE, ctypes.c_char_p, ctypes.c_int, ctypes.c_int]
send.restype = ctypes.c_int

# int recv(
#   SOCKET s,
#   char   *buf,
#   int    len,
#   int    flags
# );
recv = ctypes.windll.ws2_32.recv
recv.argtypes = [ctypes.wintypes.HANDLE, ctypes.c_char_p, ctypes.c_int, ctypes.c_int]
recv.restype = ctypes.c_int

# int closesocket(
#   IN SOCKET s
# );
closesocket = ctypes.windll.ws2_32.closesocket
closesocket.argtypes = [ctypes.wintypes.HANDLE]
closesocket.restype = ctypes.c_int

# int WSAGetLastError();
WSAGetLastError = ctypes.windll.ws2_32.WSAGetLastError
WSAGetLastError.argtypes = []
WSAGetLastError.restype = ctypes.c_int


class SOCKADDR_UN(ctypes.Structure):
    _fields_ = [("sun_family", ctypes.c_ushort), ("sun_path", ctypes.c_char * 108)]


class WindowsSocketException(Exception):
    def __init__(self, code):
        # type: (int) -> None
        super(WindowsSocketException, self).__init__(
            "Windows Socket Error: {}".format(code)
        )


class WindowsSocketHandle(object):
    AF_UNIX = 1
    SOCK_STREAM = 1

    fd = -1  # type: int
    address = ""  # type: str

    @staticmethod
    def _checkReturnCode(retcode):
        if retcode == -1:
            errcode = WSAGetLastError()
            if errcode == WSAECONNREFUSED:
                # This error will be returned when Edenfs is not running
                raise TTransportException(
                    type=TTransportException.NOT_OPEN, message="eden not running"
                )
            else:
                raise WindowsSocketException(errcode)

    def __init__(self):
        wsa_data = WSAData64()
        # ctypes.c_ushort(514) = MAKE_WORD(2,2) which is for the winsock
        # library version 2.2
        errcode = WSAStartup(ctypes.c_ushort(514), ctypes.pointer(wsa_data))
        if errcode != 0:
            raise WindowsSocketException(errcode)

        fd = socket(self.AF_UNIX, self.SOCK_STREAM, 0)
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
            connect(self.fd, ctypes.pointer(addr), ctypes.sizeof(addr))
        )
        self.address = address

    def send(self, buff):
        # type: (bytes) -> int
        retcode = send(self.fd, buff, len(buff), 0)
        self._checkReturnCode(retcode)
        return retcode

    def recv(self, size):
        # type: (int) -> bytes
        buff = ctypes.create_string_buffer(size)
        self._checkReturnCode(recv(self.fd, buff, size, 0))
        return buff.raw

    def getpeername(self):
        # type: () -> str
        return self.address

    def getsockname(self):
        # type: () -> str
        return self.address

    def close(self):
        # type: () -> int
        return closesocket(self.fd)


class WinTSocket(TSocket):
    @property
    def _shouldUseWinsocket(self):
        # type: () -> bool
        return os.name == "nt" and self._unix_socket

    def open(self):
        # type: () -> None
        # if we are not on Windows or the socktype is not unix socket, return
        # the parent TSocket
        if not self._shouldUseWinsocket:
            return super(WinTSocket, self).open()

        handle = WindowsSocketHandle()
        self.setHandle(handle)
        try:
            handle.connect(self._unix_socket)
        except Exception:
            self.close()
            raise

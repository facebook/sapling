# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This module will be shared with other services. Therefore, please refrain from
# importing anything from Mercurial and creating a dependency on Mercurial.

from __future__ import absolute_import

import json
import struct
import sys
from typing import Any, BinaryIO, Dict, List, Optional


if sys.version_info[0] >= 3:
    unicode = str


def serialize(paramsdict):
    # type (Dict[str, Any]) -> bytes
    """serialized data is formatted as follows:

    <json len: 4 byte unsigned int>
    <json>
    <fileinfo list len: 4 byte unsigned int>
    [<fileinfo>, ...]

    fileinfo = <filepath len: 4 byte unsigned int>
               <filepath>
               <file content len: 4 byte unsigned int>
               <file content>
    """

    def packunsignedint(i: int) -> bytes:
        return struct.pack("!I", i)

    def packdata(data: "Any", utf8encode: bool = True) -> "List[bytes]":
        if utf8encode:
            data = data.encode("utf-8")
        return [packunsignedint(len(data)), data]

    # Need to move the content out of the JSON representation because JSON can't
    # handle binary data.
    fileout: "List[bytes]" = []
    numfiles: int = 0
    for path, fileinfo in paramsdict["changelist"]["files"].items():
        content: "Optional[bytes]" = fileinfo["content"]
        if content:
            fileout.extend(packdata(path))
            fileout.extend(packdata(content, utf8encode=False))
            numfiles += 1
            del fileinfo["content"]

    # Now that we have excluded the content from the dictionary, we can convert
    # it to JSON.
    jsonstr: str = json.dumps(paramsdict)
    out: "List[bytes]" = packdata(jsonstr)

    # Add the information about the file contents as well.
    out.append(packunsignedint(numfiles))
    out.extend(fileout)
    return b"".join(out)


def deserialize(inputstream: "BinaryIO") -> "Dict[str, Any]":
    """deserialize inputstream to dictionary representing memcommit parameters"""

    def readexactly(stream: "BinaryIO", n: int) -> bytes:
        """read n bytes from stream.read and abort if less was available"""
        s: bytes = stream.read(n)
        if len(s) < n:
            raise EOFError(
                "stream ended unexpectedly" " (got %d bytes, expected %d)" % (len(s), n)
            )
        return s

    def readunpack(stream: "BinaryIO", fmt: str) -> "Any":
        data = readexactly(stream, struct.calcsize(fmt))  # type: bytes
        return struct.unpack(fmt, data)

    def readunsignedint(stream: "BinaryIO") -> int:
        return readunpack(stream, "!I")[0]

    def unpackdata(stream: "BinaryIO", utf8decode: bool = True) -> "Any":
        data = readexactly(stream, readunsignedint(stream))
        if utf8decode:
            return data.decode("utf-8")
        return data

    def tobytes(data: str) -> bytes:
        if isinstance(data, unicode):
            return data.encode("utf-8")

        if isinstance(data, list):
            return [tobytes(item) for item in data]

        if isinstance(data, dict):
            return {tobytes(key): tobytes(value) for key, value in data.items()}

        return data

    if sys.version_info[0] < 3:
        d: "Dict[str, Any]" = json.loads(unpackdata(inputstream), object_hook=tobytes)
    else:
        d: "Dict[str, Any]" = json.loads(unpackdata(inputstream))

    numfiles: int = readunsignedint(inputstream)
    contents: "Dict[str, bytes]" = {}
    for _ in range(0, numfiles):
        path: str = unpackdata(inputstream)
        content: bytes = unpackdata(inputstream, utf8decode=False)
        contents[path] = content

    for path, fileinfo in d["changelist"]["files"].items():
        if path in contents:
            fileinfo["content"] = contents[path]

    return d

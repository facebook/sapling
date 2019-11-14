# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This module will be shared with other services. Therefore, please refrain from
# importing anything from Mercurial and creating a dependency on Mercurial.

from __future__ import absolute_import

import json
import struct
import sys


def serialize(paramsdict):
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

    def packunsignedint(i):
        return struct.pack("!I", i)

    def packdata(data, utf8encode=True):
        if utf8encode:
            data = data.encode("utf-8")
        return [packunsignedint(len(data)), data]

    # Need to move the content out of the JSON representation because JSON can't
    # handle binary data.
    fileout = []
    numfiles = 0
    for path, fileinfo in paramsdict["changelist"]["files"].items():
        content = fileinfo["content"]
        if content:
            fileout.extend(packdata(path))
            fileout.extend(packdata(content, utf8encode=False))
            numfiles += 1
            del fileinfo["content"]

    # Now that we have excluded the content from the dictionary, we can convert
    # it to JSON.
    jsonstr = json.dumps(paramsdict)
    out = packdata(jsonstr)

    # Add the information about the file contents as well.
    out.append(packunsignedint(numfiles))
    out.extend(fileout)
    return b"".join(out)


def deserialize(inputstream):
    """ deserialize inputstream to dicttionary representing memcommit parameters
    """

    def readexactly(stream, n):
        """read n bytes from stream.read and abort if less was available"""
        s = stream.read(n)
        if len(s) < n:
            raise EOFError(
                "stream ended unexpectedly" " (got %d bytes, expected %d)" % (len(s), n)
            )
        return s

    def readunpack(stream, fmt):
        data = readexactly(stream, struct.calcsize(fmt))
        return struct.unpack(fmt, data)

    def readunsignedint(stream):
        return readunpack(stream, "!I")[0]

    def unpackdata(stream, utf8decode=True):
        data = readexactly(stream, readunsignedint(stream))
        if utf8decode:
            data = data.decode("utf-8")
        return data

    def tobytes(data):
        if isinstance(data, unicode):
            return data.encode("utf-8")

        if isinstance(data, list):
            return [tobytes(item) for item in data]

        if isinstance(data, dict):
            return {tobytes(key): tobytes(value) for key, value in data.items()}

        return data

    if sys.version_info[0] < 3:
        d = json.loads(unpackdata(inputstream), object_hook=tobytes)
    else:
        d = json.loads(unpackdata(inputstream))

    numfiles = readunsignedint(inputstream)
    contents = {}
    for _ in range(0, numfiles):
        path = unpackdata(inputstream)
        content = unpackdata(inputstream, utf8decode=False)
        contents[path] = content

    for path, fileinfo in d["changelist"]["files"].items():
        if path in contents:
            fileinfo["content"] = contents[path]

    return d

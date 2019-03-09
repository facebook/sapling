# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# This module will be shared with other services. Therefore, please refrain from
# importing anything from Mercurial and creating a dependency on Mercurial. This
# module is only for specifying classes based on simple types to represent the
# data required for creating commits.

from __future__ import absolute_import

import json
import struct
import sys


class params(object):
    def __init__(self, changelist, metadata, destination):
        self.changelist = changelist
        self.metadata = metadata
        self.destination = destination

    def _todict(self):
        d = {}
        d["changelist"] = self.changelist.todict()
        d["metadata"] = self.metadata.todict()
        d["destination"] = self.destination.todict()
        return d

    def serialize(self):
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
        d = self._todict()

        def packunsignedint(i):
            return struct.pack("!I", i)

        def packdata(data, utf8encode=True):
            if utf8encode:
                data = data.encode("utf-8")
            return [packunsignedint(len(data)), data]

        # Need to move the content out of the JSON representation because JSON
        # can't handle binary data.
        fileout = []
        numfiles = 0
        for path, fileinfo in d["changelist"]["files"].items():
            content = fileinfo["content"]
            if content:
                fileout.extend(packdata(path))
                fileout.extend(packdata(content, utf8encode=False))
                numfiles += 1
                del fileinfo["content"]

        # Now that we have excluded the content from the dictionary, we can
        # convert it to JSON.
        jsonstr = json.dumps(d)
        out = packdata(jsonstr)

        # Add the information about the file contents as well.
        out.append(packunsignedint(numfiles))
        out.extend(fileout)
        return b"".join(out)

    @classmethod
    def _fromdict(cls, d):
        return cls(
            changelist=changelist.fromdict(d["changelist"]),
            metadata=metadata.fromdict(d["metadata"]),
            destination=destination.fromdict(d["destination"]),
        )

    @classmethod
    def deserialize(cls, inputstream):
        def readexactly(stream, n):
            """read n bytes from stream.read and abort if less was available"""
            s = stream.read(n)
            if len(s) < n:
                raise EOFError(
                    "stream ended unexpectedly"
                    " (got %d bytes, expected %d)" % (len(s), n)
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

        return cls._fromdict(d)


class metadata(object):
    def __init__(self, author, description, parents, extra=None):
        self.author = author
        self.description = description
        self.parents = parents
        self.extra = extra

    def todict(self):
        d = {}
        d["author"] = self.author
        d["description"] = self.description
        d["parents"] = self.parents
        d["extra"] = self.extra
        return d

    @classmethod
    def fromdict(cls, d):
        author = d.get("author")
        description = d.get("description")
        parents = d.get("parents")
        extra = d.get("extra")
        return cls(author, description, parents, extra)


class destination(object):
    def __init__(self, bookmark=None, pushrebase=False):
        self.bookmark = bookmark
        self.pushrebase = pushrebase

    def todict(self):
        d = {}
        d["bookmark"] = self.bookmark
        d["pushrebase"] = self.pushrebase
        return d

    @classmethod
    def fromdict(cls, d):
        bookmark = d.get("bookmark")
        pushrebase = d.get("pushrebase")
        return cls(bookmark, pushrebase)


class changelistbuilder(object):
    def __init__(self, parent):
        self.parent = parent
        self.files = {}

    def addfile(self, path, fileinfo):
        self.files[path] = fileinfo

    def build(self):
        return changelist(self.parent, self.files)


class changelist(object):
    def __init__(self, parent, files):
        self.parent = parent
        self.files = files

    def todict(self):
        d = {}
        d["parent"] = self.parent
        d["files"] = {path: info.todict() for path, info in self.files.items()}
        return d

    @classmethod
    def fromdict(cls, d):
        parent = d.get("parent")
        files = {path: fileinfo.fromdict(info) for path, info in d.get("files").items()}
        return cls(parent, files)


class fileinfo(object):
    def __init__(self, deleted=False, flags=None, content=None, copysource=None):
        self.deleted = deleted
        self.flags = flags
        self.content = content
        self.copysource = copysource

    def islink(self):
        return "l" in self.flags

    def isexec(self):
        return "x" in self.flags

    def todict(self):
        d = {}
        d["deleted"] = self.deleted
        d["flags"] = self.flags
        d["content"] = self.content
        d["copysource"] = self.copysource
        return d

    @classmethod
    def fromdict(cls, d):
        deleted = d.get("deleted")
        flags = d.get("flags")
        content = d.get("content")
        copysource = d.get("copysource")
        return cls(deleted, flags, content, copysource)

# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This module will be shared with other services. Therefore, please refrain from
# importing anything from Mercurial and creating a dependency on Mercurial. This
# module is only for specifying classes based on simple types to represent the
# data required for creating commits.

from __future__ import absolute_import


class params(object):
    def __init__(self, changelist, metadata, destination):
        self.changelist = changelist
        self.metadata = metadata
        self.destination = destination

    def todict(self):
        d = {}
        d["changelist"] = self.changelist.todict()
        d["metadata"] = self.metadata.todict()
        d["destination"] = self.destination.todict()
        return d

    @classmethod
    def fromdict(cls, d):
        return cls(
            changelist=changelist.fromdict(d["changelist"]),
            metadata=metadata.fromdict(d["metadata"]),
            destination=destination.fromdict(d["destination"]),
        )


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

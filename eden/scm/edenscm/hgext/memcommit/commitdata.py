# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# This module will be shared with other services. Therefore, please refrain from
# importing anything from Mercurial and creating a dependency on Mercurial. This
# module is only for specifying classes based on simple types to represent the
# data required for creating commits.

from __future__ import absolute_import

from typing import Any, Dict, List, Optional, Union


class params(object):
    def __init__(self, changelist, metadata, destination):
        # type: (changelist, metadata, destination) -> None
        self.changelist = changelist
        self.metadata = metadata
        self.destination = destination

    def todict(self):
        # type: () -> Dict[str, Any]
        d = {}
        d["changelist"] = self.changelist.todict()
        d["metadata"] = self.metadata.todict()
        d["destination"] = self.destination.todict()
        return d

    @classmethod
    def fromdict(cls, d):
        # type: (Dict[str, Any]) -> params
        return cls(
            changelist=changelist.fromdict(d["changelist"]),
            metadata=metadata.fromdict(d["metadata"]),
            destination=destination.fromdict(d["destination"]),
        )


class metadata(object):
    def __init__(self, author, description, parents, extra=None):
        # type: (Optional[str], Optional[str], Optional[List[str]], Optional[Dict[str,str]]) -> None
        self.author = author
        self.description = description
        self.parents = parents
        self.extra = extra

    def todict(self):
        # type: () -> Dict[str, Any]
        d = {}
        d["author"] = self.author
        d["description"] = self.description
        d["parents"] = self.parents
        d["extra"] = self.extra
        return d

    @classmethod
    def fromdict(cls, d):
        # type: (Dict[str, Any]) -> metadata
        author = d.get("author")  # type: Optional[str]
        description = d.get("description")  # type: Optional[str]
        parents = d.get("parents")  # type: Optional[List[str]]
        extra = d.get("extra")  # type: Optional[Dict[str,str]]
        return cls(author, description, parents, extra)


class destination(object):
    def __init__(self, bookmark=None, pushrebase=False):
        # type: (Optional[str], Optional[bool]) -> None
        self.bookmark = bookmark
        self.pushrebase = pushrebase

    def todict(self):
        # type: () -> Dict[str, Any]
        d = {}
        d["bookmark"] = self.bookmark
        d["pushrebase"] = self.pushrebase
        return d

    @classmethod
    def fromdict(cls, d):
        # type: (Dict[str, Any]) -> destination
        bookmark = d.get("bookmark")  # type: Optional[str]
        pushrebase = d.get("pushrebase")  # type: Optional[bool]
        return cls(bookmark, pushrebase)


class changelistbuilder(object):
    def __init__(self, parent):
        # type: (str) -> None
        self.parent = parent
        self.files = {}  # type: Dict[str, fileinfo]

    def addfile(self, path, fileinfo):
        # type: (str, fileinfo) -> None
        self.files[path] = fileinfo

    def build(self):
        # type: () -> changelist
        return changelist(self.parent, self.files)


class changelist(object):
    def __init__(self, parent, files):
        # type: (Optional[str], Dict[str, fileinfo]) -> None
        self.parent = parent
        self.files = files

    def todict(self):
        # type: () -> Dict[str, Any]
        d = {}
        d["parent"] = self.parent
        d["files"] = {path: info.todict() for path, info in self.files.items()}
        return d

    @classmethod
    def fromdict(cls, d):
        # type: (Dict[str, Any]) -> changelist
        parent = d.get("parent")  # type: Optional[str]
        files = {
            path: fileinfo.fromdict(info) for path, info in d["files"].items()
        }  # type: Dict[str, fileinfo]
        return cls(parent, files)


class fileinfo(object):
    def __init__(self, deleted=False, flags=None, content=None, copysource=None):
        # type: (Optional[bool], Optional[str], Optional[str], Optional[str]) -> None
        self.deleted = deleted
        self.flags = flags
        self.content = content
        self.copysource = copysource

    def islink(self):
        # type: () -> bool
        flags = self.flags
        return flags is not None and "l" in flags

    def isexec(self):
        # type: () -> bool
        flags = self.flags
        return flags is not None and "x" in flags

    def todict(self):
        # type: () -> Dict[str, Union[bool, str]]
        d = {}
        d["deleted"] = self.deleted
        d["flags"] = self.flags
        d["content"] = self.content
        d["copysource"] = self.copysource
        return d

    @classmethod
    def fromdict(cls, d):
        # type: (Dict[str, Any]) -> fileinfo
        deleted = d.get("deleted")  # type: Optional[bool]
        flags = d.get("flags")  # type: Optional[str]
        content = d.get("content")  # type: Optional[str]
        copysource = d.get("copysource")  # type: Optional[str]
        return cls(deleted, flags, content, copysource)

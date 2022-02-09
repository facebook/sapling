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
    def __init__(
        self, changelist: "changelist", metadata: "metadata", destination: "destination"
    ) -> None:
        self.changelist = changelist
        self.metadata = metadata
        self.destination = destination

    def todict(self) -> "Dict[str, Any]":
        d = {}
        d["changelist"] = self.changelist.todict()
        d["metadata"] = self.metadata.todict()
        d["destination"] = self.destination.todict()
        return d

    @classmethod
    def fromdict(cls, d: "Dict[str, Any]") -> "params":
        return cls(
            changelist=changelist.fromdict(d["changelist"]),
            metadata=metadata.fromdict(d["metadata"]),
            destination=destination.fromdict(d["destination"]),
        )


class metadata(object):
    def __init__(
        self,
        author: "Optional[str]",
        description: "Optional[str]",
        parents: "Optional[List[str]]",
        extra: "Optional[Dict[str, str]]" = None,
    ) -> None:
        self.author = author
        self.description = description
        self.parents = parents
        self.extra = extra

    def todict(self) -> "Dict[str, Any]":
        d = {}
        d["author"] = self.author
        d["description"] = self.description
        d["parents"] = self.parents
        d["extra"] = self.extra
        return d

    @classmethod
    def fromdict(cls, d: "Dict[str, Any]") -> "metadata":
        author = d.get("author")  # type: Optional[str]
        description: "Optional[str]" = d.get("description")
        parents: "Optional[List[str]]" = d.get("parents")
        extra: "Optional[Dict[str, str]]" = d.get("extra")
        return cls(author, description, parents, extra)


class destination(object):
    def __init__(
        self, bookmark: "Optional[str]" = None, pushrebase: "Optional[bool]" = False
    ) -> None:
        self.bookmark = bookmark
        self.pushrebase = pushrebase

    def todict(self) -> "Dict[str, Any]":
        d = {}
        d["bookmark"] = self.bookmark
        d["pushrebase"] = self.pushrebase
        return d

    @classmethod
    def fromdict(cls, d: "Dict[str, Any]") -> "destination":
        bookmark = d.get("bookmark")  # type: Optional[str]
        pushrebase: "Optional[bool]" = d.get("pushrebase")
        return cls(bookmark, pushrebase)


class changelistbuilder(object):
    def __init__(self, parent: str) -> None:
        self.parent = parent
        self.files: "Dict[str, fileinfo]" = {}

    def addfile(self, path: str, fileinfo: "fileinfo") -> None:
        self.files[path] = fileinfo

    def build(self) -> "changelist":
        return changelist(self.parent, self.files)


class changelist(object):
    def __init__(self, parent: "Optional[str]", files: "Dict[str, fileinfo]") -> None:
        self.parent = parent
        self.files = files

    def todict(self) -> "Dict[str, Any]":
        d = {}
        d["parent"] = self.parent
        d["files"] = {path: info.todict() for path, info in self.files.items()}
        return d

    @classmethod
    def fromdict(cls, d: "Dict[str, Any]") -> "changelist":
        parent = d.get("parent")  # type: Optional[str]
        files: "Dict[str, fileinfo]" = {
            path: fileinfo.fromdict(info) for path, info in d["files"].items()
        }
        return cls(parent, files)


class fileinfo(object):
    def __init__(
        self,
        deleted: "Optional[bool]" = False,
        flags: "Optional[str]" = None,
        content: "Optional[str]" = None,
        copysource: "Optional[str]" = None,
    ) -> None:
        self.deleted = deleted
        self.flags = flags
        self.content = content
        self.copysource = copysource

    def islink(self) -> bool:
        flags = self.flags
        return flags is not None and "l" in flags

    def isexec(self) -> bool:
        flags = self.flags
        return flags is not None and "x" in flags

    def todict(self) -> "Dict[str, Union[bool, str]]":
        d = {}
        d["deleted"] = self.deleted
        d["flags"] = self.flags
        d["content"] = self.content
        d["copysource"] = self.copysource
        return d

    @classmethod
    def fromdict(cls, d: "Dict[str, Any]") -> "fileinfo":
        deleted = d.get("deleted")  # type: Optional[bool]
        flags: "Optional[str]" = d.get("flags")
        content: "Optional[str]" = d.get("content")
        copysource: "Optional[str]" = d.get("copysource")
        return cls(deleted, flags, content, copysource)

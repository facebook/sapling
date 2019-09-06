# -*- coding: utf-8 -*-

# metadata.py
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import hashlib

from edenscm.mercurial import error, extensions, json, pathutil
from edenscm.mercurial.i18n import _


lfs = None


def extsetup(ui):
    # TODO(alexeyqu): remove lfs requirement
    global lfs
    try:
        lfs = extensions.find("lfs")
    except KeyError:
        raise error.Abort(_("snapshot extension requires lfs to be enabled\n"))


def checkloadblobbyoid(repo, oid, path, allow_remote=False):
    localstore = repo.svfs.snapshotstore
    if localstore.has(oid):
        return
    if allow_remote:
        p = lfs.pointer.gitlfspointer(oid=oid)
        repo.svfs.lfsremoteblobstore.readbatch([p], localstore)
    else:
        raise error.Abort(
            _("file %s with oid %s not found in local blobstorage\n") % (path, oid)
        )


class filelfswrapper(object):
    """
    Helper class that links files to oids in the lfs blobstorage.
    Also does serialization/deserialization for metadata.
    """

    def __init__(self, path, oid=None, size=None):
        self.path = path
        self.oid = oid
        self.size = size

    def serialize(self):
        if not self.oid and not self.size:
            return None
        return {"oid": self.oid, "size": self.size}

    @classmethod
    def deserialize(cls, path, data):
        try:
            return cls(path, data["oid"], data["size"])
        except ValueError:
            raise error.Abort(_("invalid file description: %s\n") % data)


class snapshotmetadata(object):
    """
    Main class that contains snapshot metadata representation.
    """

    VERSION = 1

    def __init__(self, repo, oid=None):
        self.repo = repo
        self.oid = oid
        self.deleted = []
        self.unknown = []
        self.localvfsfiles = []

    @property
    def empty(self):
        return not (self.deleted or self.unknown or self.localvfsfiles)

    def serialize(self):
        files = {}
        files["deleted"] = {d.path: d.serialize() for d in self.deleted}
        files["unknown"] = {u.path: u.serialize() for u in self.unknown}
        files["localvfsfiles"] = {f.path: f.serialize() for f in self.localvfsfiles}
        metadata = {"files": files, "version": str(snapshotmetadata.VERSION)}
        return json.dumps(metadata)

    def deserialize(self, json_string):
        try:
            metadata = json.loads(json_string)
            # check version of metadata
            try:
                version = int(metadata["version"])
            except ValueError:
                raise error.Abort(
                    _("invalid metadata version: %s\n") % metadata["version"]
                )
            if version != snapshotmetadata.VERSION:
                raise error.Abort(_("invalid version number %d") % version)
            files = metadata["files"]
            self.deleted = [
                filelfswrapper(path) for path in sorted(files["deleted"].keys())
            ]
            self.unknown = [
                filelfswrapper.deserialize(path, data)
                for path, data in sorted(files["unknown"].items())
            ]
            self.localvfsfiles = [
                filelfswrapper.deserialize(path, data)
                for path, data in sorted(files["localvfsfiles"].items())
            ]
        except ValueError:
            raise error.Abort(_("invalid metadata json: %s\n") % json_string)

    def getauxfileids(self):
        auxfileids = set()
        auxfileids.update(f.oid for f in self.unknown)
        auxfileids.update(f.oid for f in self.localvfsfiles)
        return auxfileids

    @classmethod
    def createfromworkingcopy(cls, repo, status=None, include_untracked=True):
        metadata = cls(repo)
        # populate the metadata
        status = status or repo.status(unknown=include_untracked)
        metadata.deleted = [filelfswrapper(path) for path in status.deleted]
        metadata.unknown = [filelfswrapper(path) for path in status.unknown]
        # check merge and rebase info
        ismergestate = len(repo[None].parents()) > 1
        isrebasestate = repo.localvfs.exists("rebasestate")
        if ismergestate or isrebasestate:
            for root, dirs, files in repo.localvfs.walk(path="merge"):
                metadata.localvfsfiles += [
                    filelfswrapper(pathutil.join(root, f)) for f in files
                ]
        if isrebasestate:
            metadata.localvfsfiles.append(filelfswrapper("rebasestate"))
        return metadata

    @classmethod
    def getfromlocalstorage(cls, repo, oid, allow_remote=False):
        metadata = cls(repo, oid)
        checkloadblobbyoid(repo, oid, "metadata", allow_remote)
        metadata.deserialize(repo.svfs.snapshotstore.read(oid))
        # validate related files
        for file in metadata.unknown:
            checkloadblobbyoid(repo, file.oid, file.path, allow_remote)
        for file in metadata.localvfsfiles:
            checkloadblobbyoid(repo, file.oid, file.path, allow_remote)
        return metadata

    def localstore(self):
        def store(repo, data):
            """
            Util function which uploads data to the local lfs storage.
            Returns oid and size of data.
            """
            # TODO(alexeyqu): do we care about metadata?
            oid = hashlib.sha256(data).hexdigest()
            repo.svfs.snapshotstore.write(oid, data)
            return oid, str(len(data))

        wctx = self.repo[None]
        for f in self.unknown:
            f.oid, f.size = store(self.repo, wctx[f.path].data())
        for f in self.localvfsfiles:
            f.oid, f.size = store(
                self.repo, self.repo.localvfs.open(path=f.path).read()
            )
        oid, size = store(self.repo, self.serialize())
        return oid, size

    def uploadtoremotelfs(self):
        def checkgetpointer(repo, file, pointers):
            checkloadblobbyoid(repo, file.oid, file.path)
            pointers.append(lfs.pointer.gitlfspointer(oid=file.oid, size=file.size))

        assert self.oid is not None
        # TODO(alexeyqu): remove this hack in the next diff
        lfslocalstore = self.repo.svfs.lfslocalblobstore
        self.repo.svfs.lfslocalblobstore = self.repo.svfs.snapshotstore
        pointers = [lfs.pointer.gitlfspointer(oid=self.oid)]
        for file in self.unknown:
            checkgetpointer(self.repo, file, pointers)
        for file in self.localvfsfiles:
            checkgetpointer(self.repo, file, pointers)
        lfs.wrapper.uploadblobs(self.repo, pointers)
        self.repo.svfs.lfslocalblobstore = lfslocalstore

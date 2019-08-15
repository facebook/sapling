# -*- coding: utf-8 -*-

# snapshot.py
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""extension to snapshot the working copy

With this extension, Mercurial will get a set of commands
for working with full snapshots of the working copy,
including the untracked files and unresolved merge artifacts.

TODO(alexeyqu): finish docs
"""

import hashlib
import os
from collections import defaultdict

from edenscm.mercurial import (
    cmdutil,
    context,
    error,
    extensions,
    json,
    registrar,
    scmutil,
    visibility,
)
from edenscm.mercurial.i18n import _


cmdtable = {}
command = registrar.command(cmdtable)
lfs = None


def extsetup(ui):
    global lfs
    try:
        lfs = extensions.find("lfs")
    except KeyError:
        ui.warning("snapshot extension requires lfs to be enabled")


def checkloadblobbyoid(repo, oid, path, allow_remote=False):
    localstore = repo.svfs.lfslocalblobstore
    if localstore.has(oid):
        return
    if allow_remote:
        p = lfs.pointer.gitlfspointer(oid=oid)
        repo.svfs.lfsremoteblobstore.readbatch([p], localstore)
    else:
        raise error.Abort(
            _("file %s with oid %s not found in local blobstorage") % (path, oid)
        )


class filelfswrapper(object):
    """
    Helper class that links files to oids in the lfs blobstorage.
    Also does serialization/deserialization for manifest.
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
            raise error.Abort(_("invalid file description: %s") % data)


class snapshotmanifest(object):
    """
    Main class that contains snapshot manifest representation.
    """

    def __init__(self, repo, oid=None):
        self.repo = repo
        self.oid = oid
        self.deleted = []
        self.unknown = []
        self.localvfsfiles = []

    @property
    def empty(self):
        return not (self.deleted or self.unknown)

    def serialize(self):
        manifest = defaultdict(dict)
        manifest["deleted"] = {d.path: d.serialize() for d in self.deleted}
        manifest["unknown"] = {u.path: u.serialize() for u in self.unknown}
        manifest["localvfsfiles"] = {f.path: f.serialize() for f in self.localvfsfiles}
        return json.dumps(manifest)

    def deserialize(self, json_string):
        try:
            manifest = json.loads(json_string)
            self.deleted = [filelfswrapper(path) for path in manifest["deleted"]]
            self.unknown = [
                filelfswrapper.deserialize(path, data)
                for path, data in manifest["unknown"].items()
            ]
            self.localvfsfiles = [
                filelfswrapper.deserialize(path, data)
                for path, data in manifest["localvfsfiles"].items()
            ]
        except ValueError:
            raise error.Abort(_("invalid manifest json: %s") % json_string)

    @classmethod
    def createfromworkingcopy(cls, repo, status=None, include_untracked=True):
        manifest = cls(repo)
        # populate the manifest
        status = status or repo.status(unknown=include_untracked)
        manifest.deleted = [filelfswrapper(path) for path in status.deleted]
        manifest.unknown = [filelfswrapper(path) for path in status.unknown]
        # check merge and rebase info
        ismergestate = len(repo[None].parents()) > 1
        isrebasestate = repo.localvfs.exists("rebasestate")
        if ismergestate or isrebasestate:
            for root, dirs, files in repo.localvfs.walk(path="merge"):
                manifest.localvfsfiles += [
                    filelfswrapper(os.path.join(root, f)) for f in files
                ]
        if isrebasestate:
            manifest.localvfsfiles.append(filelfswrapper("rebasestate"))
        return manifest

    @classmethod
    def restorefromlfs(cls, repo, oid, allow_remote=False):
        manifest = cls(repo, oid)
        checkloadblobbyoid(repo, oid, "manifest", allow_remote)
        manifest.deserialize(repo.svfs.lfslocalblobstore.read(oid))
        # validate related files
        for file in manifest.unknown:
            checkloadblobbyoid(repo, file.oid, file.path, allow_remote)
        for file in manifest.localvfsfiles:
            checkloadblobbyoid(repo, file.oid, file.path, allow_remote)
        return manifest

    def storetolocallfs(self):
        def storetolfs(repo, data):
            """
            Util function which uploads data to the local lfs storage.
            Returns oid and size of data.
            """
            # TODO(alexeyqu): do we care about metadata?
            oid = hashlib.sha256(data).hexdigest()
            repo.svfs.lfslocalblobstore.write(oid, data)
            return oid, str(len(data))

        wctx = self.repo[None]
        for f in self.unknown:
            f.oid, f.size = storetolfs(self.repo, wctx[f.path].data())
        for f in self.localvfsfiles:
            f.oid, f.size = storetolfs(
                self.repo, self.repo.localvfs.open(path=f.path).read()
            )
        oid, size = storetolfs(self.repo, self.serialize())
        return oid, size

    def uploadtoremotelfs(self):
        def checkgetpointer(repo, file, pointers):
            checkloadblobbyoid(repo, file.oid, file.path)
            pointers.append(lfs.pointer.gitlfspointer(oid=file.oid, size=file.size))

        assert self.oid is not None
        pointers = [lfs.pointer.gitlfspointer(oid=self.oid)]
        for file in self.unknown:
            checkgetpointer(self.repo, file, pointers)
        for file in self.localvfsfiles:
            checkgetpointer(self.repo, file, pointers)
        lfs.wrapper.uploadblobs(self.repo, pointers)


@command("debugsnapshot", inferrepo=True)
def debugsnapshot(ui, repo, *args, **opts):
    """
    Creates a snapshot of the working copy.
    TODO(alexeyqu): finish docs
    """
    if lfs is None:
        raise error.Abort(_("lfs is not initialised"))
    with repo.wlock(), repo.lock():
        node = createsnapshotcommit(ui, repo, opts)
        if not node:
            ui.status(_("nothing changed\n"))
            return
        ui.status(_("snapshot %s created\n") % (repo[node].hex()))
        if visibility.enabled(repo):
            visibility.remove(repo, [node])


def createsnapshotcommit(ui, repo, opts):
    status = repo.status(unknown=True)
    snapmanifest = snapshotmanifest.createfromworkingcopy(repo, status=status)
    emptymanifest = snapmanifest.empty
    oid = None
    if not emptymanifest:
        oid, size = snapmanifest.storetolocallfs()
    extra = {"snapshotmanifestid": oid}
    ui.debug("snapshot extra %s\n" % extra)
    # TODO(alexeyqu): deal with unfinished merge state case
    cctx = context.workingcommitctx(
        repo, status, "snapshot", opts.get("user"), opts.get("date"), extra=extra
    )
    if len(cctx.files()) == 0 and emptymanifest:  # don't need an empty snapshot
        return None
    with repo.transaction("snapshot"):
        return repo.commitctx(cctx, error=True)


@command("debugcreatesnapshotmanifest", inferrepo=True)
def debugcreatesnapshotmanifest(ui, repo, *args, **opts):
    """
    Creates pseudo manifest for untracked files without committing them.
    Loads untracked files and the created manifest into local lfsstore.
    Outputs the oid of the created manifest file.

    Be careful, snapshot manifest internal structure may change.
    """
    if lfs is None:
        raise error.Abort(_("lfs is not initialised"))
    snapmanifest = snapshotmanifest.createfromworkingcopy(repo)
    if snapmanifest.empty:
        ui.status(
            _(
                "Working copy is even with the last commit. "
                "No need to create snapshot.\n"
            )
        )
        return
    oid, size = snapmanifest.storetolocallfs()
    ui.status(_("manifest oid: %s\n") % oid)


@command("debuguploadsnapshotmanifest", [], _("OID"), inferrepo=True)
def debuguploadsnapshotmanifest(ui, repo, *args, **opts):
    """
    Uploads manifest and all related blobs to remote lfs.
    Takes in an oid of the desired manifest in the local lfs.

    This command does not validate contents of the snapshot manifest.
    """
    if lfs is None:
        raise error.Abort(_("lfs not initialised"))
    if not args or len(args) != 1:
        raise error.Abort(_("you must specify a manifest oid"))
    snapmanifest = snapshotmanifest.restorefromlfs(repo, args[0])
    snapmanifest.uploadtoremotelfs()
    ui.status(_("upload complete\n"))


@command("debugcheckoutsnapshot", [], _("OID"), inferrepo=True)
def debugcheckoutsnapshot(ui, repo, *args, **opts):
    """
    Checks out the working copy to the snapshot state, given its manifest oid.
    Downloads the snapshot manifest from remote lfs if needed.
    Takes in an oid of the manifest.

    This command does not validate contents of the snapshot manifest.
    """
    if lfs is None:
        raise error.Abort(_("lfs not initialised"))
    if not args or len(args) != 1:
        raise error.Abort(_("you must specify a manifest oid"))
    snapmanifest = snapshotmanifest.restorefromlfs(repo, args[0], allow_remote=True)
    # deleting files that should be missing
    todelete = [f.path for f in snapmanifest.deleted]
    ui.note(_("will delete %s") % ",".join(todelete))
    m = scmutil.match(repo[None], todelete)
    cmdutil.remove(ui, repo, m, "", after=False, force=False)
    # populating the untracked files
    for unknown in snapmanifest.unknown:
        ui.note(_("will add %s") % unknown.path)
        repo.wvfs.write(unknown.path, repo.svfs.lfslocalblobstore.read(unknown.oid))
    # restoring the merge state
    with repo.wlock():
        for file in snapmanifest.localvfsfiles:
            ui.note(_("will add %s") % file.path)
            repo.localvfs.write(file.path, repo.svfs.lfslocalblobstore.read(file.oid))
    ui.status(_("snapshot checkout complete\n"))

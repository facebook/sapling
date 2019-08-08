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
from collections import defaultdict

from edenscm.mercurial import cmdutil, error, extensions, json, registrar, scmutil
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

    @property
    def empty(self):
        return not (self.deleted or self.unknown)

    def serialize(self):
        manifest = defaultdict(dict)
        manifest["deleted"] = {d.path: d.serialize() for d in self.deleted}
        manifest["unknown"] = {u.path: u.serialize() for u in self.unknown}
        return json.dumps(manifest)

    def deserialize(self, json_string):
        try:
            manifest = json.loads(json_string)
            self.deleted = [filelfswrapper(path) for path in manifest["deleted"]]
            self.unknown = [
                filelfswrapper.deserialize(path, data)
                for path, data in manifest["unknown"].items()
            ]
        except ValueError:
            raise error.Abort(_("invalid manifest json: %s") % json_string)

    @classmethod
    def createfromworkingcopy(cls, repo, include_untracked):
        manifest = cls(repo)
        # populate the manifest
        status = manifest.repo.status(unknown=include_untracked)
        manifest.deleted = [filelfswrapper(path) for path in status.deleted]
        manifest.unknown = [filelfswrapper(path) for path in status.unknown]
        return manifest

    @classmethod
    def restorefromlfs(cls, repo, oid, allow_remote=False):
        manifest = cls(repo, oid)
        checkloadblobbyoid(manifest.repo, oid, "manifest", allow_remote)
        manifest.deserialize(manifest.repo.svfs.lfslocalblobstore.read(oid))
        # validate related files
        for file in manifest.unknown:
            checkloadblobbyoid(manifest.repo, file.oid, file.path, allow_remote)
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
        oid, size = storetolfs(self.repo, self.serialize())
        return oid, size

    def uploadtoremotelfs(self):
        assert self.oid is not None
        pointers = [lfs.pointer.gitlfspointer(oid=self.oid)]
        for file in self.unknown:
            checkloadblobbyoid(self.repo, file.oid, file.path)
            pointers.append(lfs.pointer.gitlfspointer(oid=file.oid, size=file.size))
        lfs.wrapper.uploadblobs(self.repo, pointers)


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
    snapmanifest = snapshotmanifest.createfromworkingcopy(repo, include_untracked=True)
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
    ui.status(_("snapshot checkout complete\n"))

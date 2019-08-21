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
    hg,
    json,
    pathutil,
    registrar,
    scmutil,
    util,
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
        raise error.Abort(_("snapshot extension requires lfs to be enabled\n"))


def checkloadblobbyoid(repo, oid, path, allow_remote=False):
    localstore = repo.svfs.lfslocalblobstore
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
            raise error.Abort(_("invalid file description: %s\n") % data)


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
            self.deleted = [
                filelfswrapper(path) for path in sorted(manifest["deleted"].keys())
            ]
            self.unknown = [
                filelfswrapper.deserialize(path, data)
                for path, data in sorted(manifest["unknown"].items())
            ]
            self.localvfsfiles = [
                filelfswrapper.deserialize(path, data)
                for path, data in sorted(manifest["localvfsfiles"].items())
            ]
        except ValueError:
            raise error.Abort(_("invalid manifest json: %s\n") % json_string)

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
                    filelfswrapper(pathutil.join(root, f)) for f in files
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


@command(
    "debugsnapshot", [("", "clean", False, _("clean the working copy"))], inferrepo=True
)
def debugsnapshot(ui, repo, *args, **opts):
    """
    Creates a snapshot of the working copy.
    TODO(alexeyqu): finish docs
    """

    def removeuntrackedfiles(ui, repo):
        """
        Removes all untracked files from the repo.
        """
        # the same behavior is implemented better in the purge extension
        # more corner cases are handled there
        # e.g. directories that became empty during purge get deleted too
        # TODO(alexeyqu): use code from purge, probable move it to core code
        status = repo.status(unknown=True)
        for file in status.unknown:
            try:
                util.tryunlink(repo.wjoin(file))
            except OSError:
                ui.warn(_("%s cannot be removed") % file)

    with repo.wlock(), repo.lock():
        node = createsnapshotcommit(ui, repo, opts)
        if not node:
            ui.status(_("nothing changed\n"))
            return
        ui.status(_("snapshot %s created\n") % (repo[node].hex()))
        if visibility.enabled(repo):
            visibility.remove(repo, [node])
        if opts.get("clean"):
            try:
                # We want to bring the working copy to the p1 state
                rev = repo[None].p1()
                hg.updatetotally(ui, repo, rev, rev, clean=True)
                removeuntrackedfiles(ui, repo)
            except (KeyboardInterrupt, Exception) as exc:
                ui.warn(_("failed to clean the working copy: %s\n") % exc)


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


@command(
    "debugcheckoutsnapshot",
    [("f", "force", False, _("force checkout"))],
    _("REV"),
    inferrepo=True,
)
def debugcheckoutsnapshot(ui, repo, *args, **opts):
    """
    Checks out the working copy to the snapshot state, given its revision id.
    Downloads the snapshot manifest from remote lfs if needed.

    """
    if not args or len(args) != 1:
        raise error.Abort(_("you must specify a snapshot revision id\n"))
    force = opts.get("force")
    node = args[0]
    try:
        cctx = repo.unfiltered()[node]
    except error.RepoLookupError:
        ui.status(_("%s is not a valid revision id\n") % node)
        raise
    if "snapshotmanifestid" not in cctx.extra():
        raise error.Abort(_("%s is not a valid snapshot id\n") % node)
    # This is a temporary safety check that WC is clean.
    if sum(map(len, repo.status(unknown=True))) != 0 and not force:
        raise error.Abort(
            _(
                "You must have a clean working copy to checkout on a snapshot. "
                "Use --force to bypass that.\n"
            )
        )
    ui.status(_("will checkout on %s\n") % cctx.hex())
    with repo.wlock():
        # TODO(alexeyqu): support EdenFS and possibly make it more efficient
        hg.update(repo, node)
        with repo.dirstate.parentchange():
            newparents = [p.node() for p in cctx.parents()]
            ui.debug("setting parents to %s\n" % newparents)
            repo.setparents(*newparents)
    snapshotmanifestid = cctx.extra().get("snapshotmanifestid")
    if snapshotmanifestid:
        snapmanifest = snapshotmanifest.restorefromlfs(repo, snapshotmanifestid)
        checkouttosnapshotmanifest(ui, repo, snapmanifest, force)
    ui.status(_("checkout complete\n"))


@command("debugcreatesnapshotmanifest", inferrepo=True)
def debugcreatesnapshotmanifest(ui, repo, *args, **opts):
    """
    Creates pseudo manifest for untracked files without committing them.
    Loads untracked files and the created manifest into local lfsstore.
    Outputs the oid of the created manifest file.

    Be careful, snapshot manifest internal structure may change.
    """
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
    if not args or len(args) != 1:
        raise error.Abort(_("you must specify a manifest oid\n"))
    snapmanifest = snapshotmanifest.restorefromlfs(repo, args[0])
    snapmanifest.uploadtoremotelfs()
    ui.status(_("upload complete\n"))


@command(
    "debugcheckoutsnapshotmanifest",
    [("f", "force", False, _("force checkout"))],
    _("OID"),
    inferrepo=True,
)
def debugcheckoutsnapshotmanifest(ui, repo, *args, **opts):
    """
    Checks out the working copy to the snapshot manifest state, given its manifest oid.
    Downloads the snapshot manifest from remote lfs if needed.
    Takes in an oid of the manifest.

    This command does not validate contents of the snapshot manifest.
    """
    if not args or len(args) != 1:
        raise error.Abort(_("you must specify a manifest oid\n"))
    snapmanifest = snapshotmanifest.restorefromlfs(repo, args[0], allow_remote=True)
    checkouttosnapshotmanifest(ui, repo, snapmanifest, force=opts.get("force"))
    ui.status(_("snapshot checkout complete\n"))


def checkouttosnapshotmanifest(ui, repo, snapmanifest, force=True):
    def checkaddfile(store, file, vfs, force):
        if not force and vfs.exists(file.path):
            ui.note(_("skip adding %s, it exists\n") % file.path)
            return
        ui.note(_("will add %s\n") % file.path)
        vfs.write(file.path, store.read(file.oid))

    # deleting files that should be missing
    for file in snapmanifest.deleted:
        try:
            ui.note(_("will delete %s\n") % file.path)
            util.unlink(repo.wjoin(file.path))
        except OSError:
            ui.warn(_("%s cannot be removed\n") % file.path)
    # populating the untracked files
    for file in snapmanifest.unknown:
        checkaddfile(repo.svfs.lfslocalblobstore, file, repo.wvfs, force)
    # restoring the merge state
    with repo.wlock():
        for file in snapmanifest.localvfsfiles:
            checkaddfile(repo.svfs.lfslocalblobstore, file, repo.localvfs, force)

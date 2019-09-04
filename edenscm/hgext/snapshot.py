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

Configs::

    [ui]
    # Allow to run `hg checkout` for snapshot revisions
    allow-checkout-snapshot = False
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
    merge as mergemod,
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

configtable = {}
configitem = registrar.configitem(configtable)
configitem("ui", "allow-checkout-snapshot", default=False)


def extsetup(ui):
    global lfs
    try:
        lfs = extensions.find("lfs")
    except KeyError:
        raise error.Abort(_("snapshot extension requires lfs to be enabled\n"))

    extensions.wrapfunction(hg, "updaterepo", _updaterepo)


def _updaterepo(orig, repo, node, overwrite, **opts):
    allowsnapshots = repo.ui.configbool("ui", "allow-checkout-snapshot")
    unfi = repo.unfiltered()
    if not allowsnapshots and node in unfi:
        ctx = unfi[node]
        if "snapshotmetadataid" in ctx.extra():
            raise error.Abort(
                _(
                    "%s is a snapshot, set ui.allow-checkout-snapshot"
                    " config to True to checkout on it\n"
                )
                % ctx
            )
    return orig(repo, node, overwrite, **opts)


@command("snapshot", [], "SUBCOMMAND ...", subonly=True)
def snapshot(ui, repo, *args, **opts):
    """make a restorable snapshot the working copy state

    The snapshot extension lets you make a restorable snapshot of
    the whole working copy state at any moment. This is somewhat similar
    to shelve command, but is available anytime (e.g. in the middle of
    a merge conflict resolution).

    Use 'hg snapshot create' to create a snapshot. It will print the snapshot's id.

    Use 'hg snapshot checkout SNAPSHOT_ID' to checkout to the snapshot.
    """
    pass


subcmd = snapshot.subcommand(
    categories=[("Snapshot create/restore", ["create", "checkout"])]
)


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

    VERSION = "1"

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
        metadata = {"files": files, "version": snapshotmetadata.VERSION}
        return json.dumps(metadata)

    def deserialize(self, json_string):
        try:
            metadata = json.loads(json_string)
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
    def restorefromlfs(cls, repo, oid, allow_remote=False):
        metadata = cls(repo, oid)
        checkloadblobbyoid(repo, oid, "metadata", allow_remote)
        metadata.deserialize(repo.svfs.lfslocalblobstore.read(oid))
        # validate related files
        for file in metadata.unknown:
            checkloadblobbyoid(repo, file.oid, file.path, allow_remote)
        for file in metadata.localvfsfiles:
            checkloadblobbyoid(repo, file.oid, file.path, allow_remote)
        return metadata

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


@subcmd("create", [("", "clean", False, _("clean the working copy"))], inferrepo=True)
def snapshotcreate(ui, repo, *args, **opts):
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
    snapmetadata = snapshotmetadata.createfromworkingcopy(repo, status=status)
    emptymetadata = snapmetadata.empty
    oid = ""  # this is better than None because of extra serialization rules
    if not emptymetadata:
        oid, size = snapmetadata.storetolocallfs()
    extra = {"snapshotmetadataid": oid}
    ui.debug("snapshot extra %s\n" % extra)
    # TODO(alexeyqu): deal with unfinished merge state case
    cctx = context.workingcommitctx(
        repo, status, "snapshot", opts.get("user"), opts.get("date"), extra=extra
    )
    if len(cctx.files()) == 0 and emptymetadata:  # don't need an empty snapshot
        return None
    with repo.transaction("snapshot"):
        return repo.commitctx(cctx, error=True)


@subcmd(
    "checkout", [("f", "force", False, _("force checkout"))], _("REV"), inferrepo=True
)
def snapshotcheckout(ui, repo, *args, **opts):
    """
    Checks out the working copy to the snapshot state, given its revision id.
    Downloads the snapshot metadata from remote lfs if needed.

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
    if "snapshotmetadataid" not in cctx.extra():
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
        parents = [p.node() for p in cctx.parents()]
        # First we check out on the 1st parent of the snapshot state
        hg.update(repo.unfiltered(), parents[0], quietempty=True)
        # Then we update snapshot files in the working copy
        # Here the dirstate is not updated because of the matcher
        matcher = scmutil.match(cctx, cctx.files(), opts)
        mergemod.update(repo.unfiltered(), node, False, False, matcher=matcher)
        # Finally, we mark the modified files in the dirstate
        scmutil.addremove(repo, matcher, "", opts)
        # Tie the state to the 2nd parent if needed
        if len(parents) == 2:
            with repo.dirstate.parentchange():
                repo.setparents(*parents)
    snapshotmetadataid = cctx.extra().get("snapshotmetadataid")
    if snapshotmetadataid:
        snapmetadata = snapshotmetadata.restorefromlfs(repo, snapshotmetadataid)
        checkouttosnapshotmetadata(ui, repo, snapmetadata, force)
    ui.status(_("checkout complete\n"))


@command("debugcreatesnapshotmetadata", inferrepo=True)
def debugcreatesnapshotmetadata(ui, repo, *args, **opts):
    """
    Creates pseudo metadata for untracked files without committing them.
    Loads untracked files and the created metadata into local lfsstore.
    Outputs the oid of the created metadata file.

    Be careful, snapshot metadata internal structure may change.
    """
    snapmetadata = snapshotmetadata.createfromworkingcopy(repo)
    if snapmetadata.empty:
        ui.status(
            _(
                "Working copy is even with the last commit. "
                "No need to create snapshot.\n"
            )
        )
        return
    oid, size = snapmetadata.storetolocallfs()
    ui.status(_("metadata oid: %s\n") % oid)


@command("debuguploadsnapshotmetadata", [], _("OID"), inferrepo=True)
def debuguploadsnapshotmetadata(ui, repo, *args, **opts):
    """
    Uploads metadata and all related blobs to remote lfs.
    Takes in an oid of the desired metadata in the local lfs.

    This command does not validate contents of the snapshot metadata.
    """
    if not args or len(args) != 1:
        raise error.Abort(_("you must specify a metadata oid\n"))
    snapmetadata = snapshotmetadata.restorefromlfs(repo, args[0])
    snapmetadata.uploadtoremotelfs()
    ui.status(_("upload complete\n"))


@command(
    "debugcheckoutsnapshotmetadata",
    [("f", "force", False, _("force checkout"))],
    _("OID"),
    inferrepo=True,
)
def debugcheckoutsnapshotmetadata(ui, repo, *args, **opts):
    """
    Checks out the working copy to the snapshot metadata state, given its metadata id.
    Downloads the snapshot metadata from remote lfs if needed.
    Takes in an oid of the metadata.

    This command does not validate contents of the snapshot metadata.
    """
    if not args or len(args) != 1:
        raise error.Abort(_("you must specify a metadata oid\n"))
    snapmetadata = snapshotmetadata.restorefromlfs(repo, args[0], allow_remote=True)
    checkouttosnapshotmetadata(ui, repo, snapmetadata, force=opts.get("force"))
    ui.status(_("snapshot checkout complete\n"))


def checkouttosnapshotmetadata(ui, repo, snapmetadata, force=True):
    def checkaddfile(store, file, vfs, force):
        if not force and vfs.exists(file.path):
            ui.note(_("skip adding %s, it exists\n") % file.path)
            return
        ui.note(_("will add %s\n") % file.path)
        vfs.write(file.path, store.read(file.oid))

    # deleting files that should be missing
    for file in snapmetadata.deleted:
        try:
            ui.note(_("will delete %s\n") % file.path)
            util.unlink(repo.wjoin(file.path))
        except OSError:
            ui.warn(_("%s cannot be removed\n") % file.path)
    # populating the untracked files
    for file in snapmetadata.unknown:
        checkaddfile(repo.svfs.lfslocalblobstore, file, repo.wvfs, force)
    # restoring the merge state
    with repo.wlock():
        for file in snapmetadata.localvfsfiles:
            checkaddfile(repo.svfs.lfslocalblobstore, file, repo.localvfs, force)

# -*- coding: utf-8 -*-

# commands.py
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from edenscm.mercurial import (
    cmdutil,
    context,
    error,
    hg,
    merge as mergemod,
    registrar,
    scmutil,
    util,
    visibility,
)
from edenscm.mercurial.i18n import _

from .metadata import snapshotmetadata
from .snapshotlist import snapshotlist


cmdtable = {}
command = registrar.command(cmdtable)


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


@subcmd(
    "create",
    [
        ("", "clean", False, _("clean the working copy")),
        ("m", "message", "", _("use text as a snapshot commit message"), _("TEXT")),
    ],
    inferrepo=True,
)
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
        with repo.transaction("update-snapshot-list") as tr:
            snapshotlist(repo).add([node], tr)
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
        oid, size = snapmetadata.localstore()
    extra = {"snapshotmetadataid": oid}
    ui.debug("snapshot extra %s\n" % extra)
    # TODO(alexeyqu): deal with unfinished merge state case
    text = opts.get("message") or "snapshot"
    cctx = context.workingcommitctx(
        repo, status, text, opts.get("user"), opts.get("date"), extra=extra
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
        snapmetadata = snapshotmetadata.getfromlocalstorage(repo, snapshotmetadataid)
        checkouttosnapshotmetadata(ui, repo, snapmetadata, force)
    ui.status(_("checkout complete\n"))


@subcmd("list", cmdutil.formatteropts)
def snapshotlistcmd(ui, repo, *args, **opts):
    """list the local snapshots
    """
    snapshotlist(repo).printsnapshots(ui, **opts)


@command("debugcreatesnapshotmetadata", inferrepo=True)
def debugcreatesnapshotmetadata(ui, repo, *args, **opts):
    """
    Creates pseudo metadata for untracked files without committing them.
    Loads untracked files and the created metadata into local blobstore.
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
    oid, size = snapmetadata.localstore()
    ui.status(_("metadata oid: %s\n") % oid)


@command(
    "debugcheckoutsnapshotmetadata",
    [("f", "force", False, _("force checkout"))],
    _("OID"),
    inferrepo=True,
)
def debugcheckoutsnapshotmetadata(ui, repo, *args, **opts):
    """
    Checks out the working copy to the snapshot metadata state, given its metadata id.
    Takes in an oid of the metadata.

    This command does not validate contents of the snapshot metadata.
    """
    if not args or len(args) != 1:
        raise error.Abort(_("you must specify a metadata oid\n"))
    snapmetadata = snapshotmetadata.getfromlocalstorage(repo, args[0])
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
        checkaddfile(repo.svfs.snapshotstore, file, repo.wvfs, force)
    # restoring the merge state
    with repo.wlock():
        for file in snapmetadata.localvfsfiles:
            checkaddfile(repo.svfs.snapshotstore, file, repo.localvfs, force)

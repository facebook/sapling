# commands.py
#
# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from edenscm.mercurial import (
    cmdutil,
    context,
    error,
    extensions,
    hg,
    match as matchmod,
    mdiff,
    merge as mergemod,
    node as nodemod,
    patch,
    pycompat,
    registrar,
    scmutil,
    util,
)
from edenscm.mercurial.i18n import _

from .metadata import snapshotmetadata


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
        node = nodemod.hex(node)
        with repo.transaction("add-snapshot") as tr:
            repo.snapshotlist.update(tr, addnodes=[node])
        ui.status(_("snapshot %s created\n") % (node))
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
        oid, size = snapmetadata.storelocally(repo)
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


@subcmd("show", cmdutil.logopts, _("REV"), cmdtype=registrar.command.readonly)
def snapshotshow(ui, repo, *args, **opts):
    """show the snapshot contents, given its revision id
    """
    opts = pycompat.byteskwargs(opts)
    if not args or len(args) != 1:
        raise error.Abort(_("you must specify a snapshot revision id\n"))
    node = args[0]
    try:
        cctx = repo.unfiltered()[node]
    except error.RepoLookupError:
        ui.status(_("%s is not a valid revision id\n") % node)
        raise
    if "snapshotmetadataid" not in cctx.extra():
        raise error.Abort(_("%s is not a valid snapshot id\n") % node)
    rev = cctx.hex()
    opts["rev"] = [rev]
    opts["patch"] = True
    revs, expr, filematcher = cmdutil.getlogrevs(repo.unfiltered(), [], opts)
    revmatchfn = filematcher(rev) if filematcher else None
    ui.pager("snapshotshow")
    displayer = cmdutil.show_changeset(ui, repo.unfiltered(), opts, buffered=True)
    with extensions.wrappedfunction(patch, "diff", _diff), extensions.wrappedfunction(
        cmdutil.changeset_printer, "_show", _show
    ):
        displayer.show(cctx, matchfn=revmatchfn)
        displayer.flush(cctx)
    displayer.close()


def _diff(orig, repo, *args, **kwargs):
    def snapshotdiff(data1, data2, path):
        uheaders, hunks = mdiff.unidiff(
            data1,
            date1,
            data2,
            date2,
            path,
            path,
            opts=kwargs.get("opts"),
            check_binary=False,
        )
        return "".join(sum((list(hlines) for hrange, hlines in hunks), []))

    for text in orig(repo, *args, **kwargs):
        yield text
    node2 = kwargs.get("node2") or args[1]
    if node2 is None:
        # this should be the snapshot node
        raise StopIteration
    ctx2 = repo.unfiltered()[node2]
    date2 = util.datestr(ctx2.date())
    node1 = kwargs.get("node1") or args[0]
    if node1 is not None:
        ctx1 = repo[node1]
    else:
        # is that possible?
        ctx1 = ctx2.p1()
    date1 = util.datestr(ctx1.date())
    metadataid = ctx2.extra().get("snapshotmetadataid", "")
    if not metadataid:
        # node2 is not a snapshot
        raise StopIteration
    snapmetadata = snapshotmetadata.getfromlocalstorage(repo, metadataid)
    store = repo.svfs.snapshotstore
    # print unknown files from snapshot
    # diff("", content)
    yield "\n===\nUntracked changes:\n===\n"
    for f in snapmetadata.unknown:
        yield "? %s\n" % f.path
        yield snapshotdiff("", store.read(f.oid), f.path)
    # print deleted files from snapshot
    # diff(prevcontent, "")
    for f in snapmetadata.deleted:
        yield "! %s\n" % f.path
        fctx1 = ctx1.filectx(f.path)
        yield snapshotdiff(fctx1.data(), "", f.path)


def _getsnapshotrepostate(ctx):
    # TODO(alexeyqu): check this via snapshotlist
    metadataid = ctx.extra().get("snapshotmetadataid", "")
    if not metadataid:
        return None
    repo = ctx.repo()
    snapmetadata = snapshotmetadata.getfromlocalstorage(repo, metadataid)
    if "rebasestate" in snapmetadata.localvfsfiles:
        return "rebase"
    if len(ctx.parents()) > 1:
        return "merge"
    return None


def _show(orig, self, ctx, *args):
    orig(self, ctx, *args)
    state = _getsnapshotrepostate(ctx)
    if state:
        # TODO(alexeyqu): add more information about the state here
        self.ui.write(_("The snapshot is in an unfinished *%s* state.\n") % state)


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


@subcmd("list", cmdutil.formatteropts)
def snapshotlistcmd(ui, repo, *args, **opts):
    """list the local snapshots
    """
    repo.snapshotlist.printsnapshots(ui, repo, **opts)


@subcmd("hide", [], _("REV"))
def snapshothide(ui, repo, *args, **opts):
    """hide a snapshot: remove it from the snapshot list
    """
    if not args or len(args) != 1:
        raise error.Abort(_("you must specify a snapshot revision id\n"))
    node = args[0]
    try:
        cctx = repo.unfiltered()[node]
    except error.RepoLookupError:
        ui.status(_("%s is not a valid revision id\n") % node)
        raise
    if "snapshotmetadataid" not in cctx.extra():
        raise error.Abort(_("%s is not a valid snapshot id\n") % node)
    with repo.lock(), repo.transaction("hide-snapshot") as tr:
        repo.snapshotlist.update(tr, removenodes=[cctx.hex()])


@subcmd("unhide", [], _("REV"))
def snapshotunhide(ui, repo, *args, **opts):
    """unhide a snapshot: add it to the snapshot list
    """
    if not args or len(args) != 1:
        raise error.Abort(_("you must specify a snapshot revision id\n"))
    node = args[0]
    try:
        cctx = repo.unfiltered()[node]
    except error.RepoLookupError:
        ui.status(_("%s is not a valid revision id\n") % node)
        raise
    if "snapshotmetadataid" not in cctx.extra():
        raise error.Abort(_("%s is not a valid snapshot id\n") % node)
    with repo.lock(), repo.transaction("unhide-snapshot") as tr:
        repo.snapshotlist.update(tr, addnodes=[cctx.hex()])

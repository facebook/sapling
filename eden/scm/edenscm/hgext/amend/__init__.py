# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# amend.py - improved amend functionality

"""extends the existing commit amend functionality

Adds an hg amend command that amends the current parent changeset with the
changes in the working copy.  Similar to the existing hg commit --amend
except it doesn't prompt for the commit message unless --edit is provided.

Allows amending changesets that have children and can automatically rebase
the children onto the new version of the changeset.

To make `hg previous` and `hg next` always pick the newest commit at
each step of walking up or down the stack instead of aborting when
encountering non-linearity (equivalent to the --newest flag), enable
the following config option::

    [amend]
    alwaysnewest = true

To automatically update the commit date, enable the following config option::

    [amend]
    date = implicitupdate

Commits are restacked automatically on amend, if doing so doesn't create
conflicts. To never automatically restack::

    [amend]
    autorestack = none

Note that if --date is specified on the command line, it takes precedence.

If a split creates multiple commits that have the same phabricator diff, the
following advice for resolution will be shown::

    [split]
    phabricatoradvice = edit the commit messages to remove the association

    To make `hg next` prefer draft commits in case of ambiguity, enable the following config option:

    [update]
    nextpreferdraft = true
"""

from __future__ import absolute_import

import io

from bindings import checkout as nativecheckout
from edenscm.mercurial import (
    cmdutil,
    commands,
    context,
    error,
    extensions,
    hintutil,
    lock as lockmod,
    mutation,
    patch,
    phases,
    registrar,
    scmutil,
)
from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import short

from .. import rebase as rebasemod
from . import common, fold, hide, metaedit, movement, restack, revsets, split, unamend


revsetpredicate = revsets.revsetpredicate
hint = registrar.hint()

cmdtable = {}
command = registrar.command(cmdtable)

cmdtable.update(fold.cmdtable)
cmdtable.update(hide.cmdtable)
cmdtable.update(metaedit.cmdtable)
cmdtable.update(movement.cmdtable)
cmdtable.update(split.cmdtable)
cmdtable.update(unamend.cmdtable)

configtable = {}
configitem = registrar.configitem(configtable)

configitem("amend", "alwaysnewest", default=False)
configitem("amend", "date", default=None)
configitem("amend", "education", default=None)
configitem("commands", "amend.autorebase", default=True)
configitem("update", "nextpreferdraft", default=True)

testedwith = "ships-with-fb-hgext"

amendopts = [
    ("", "rebase", None, _("rebases children after the amend")),
    ("", "fixup", None, _("rebase children from a previous amend (DEPRECATED)")),
    ("", "to", "", _("amend to a specific commit in the current stack (ADVANCED)")),
] + cmdutil.templateopts

# Never restack commits on amend.
RESTACK_NEVER = "never"

# Restack commits on amend only if they chage manifest, and don't change the
# commit manifest.
RESTACK_ONLY_TRIVIAL = "only-trivial"

# Restack commits on amend only if doing so won't create merge conflicts.
RESTACK_NO_CONFLICT = "no-conflict"

# Always attempt to restack commits on amend, even if doing so will leave the
# user in a conflicted state.
RESTACK_ALWAYS = "always"

# Possible restack values for `amend.autorestack`.
RESTACK_VALUES = [
    RESTACK_NEVER,
    RESTACK_ONLY_TRIVIAL,
    RESTACK_NO_CONFLICT,
    RESTACK_ALWAYS,
]

RESTACK_DEFAULT = RESTACK_ONLY_TRIVIAL


@hint("strip-hide")
def hinthide():
    return _("'hg strip' may be deprecated in the future - " "use 'hg hide' instead")


@hint("strip-uncommit")
def hintstrip():
    return _(
        "'hg strip' may be deprecated in the future - "
        "use 'hg uncommit' or 'hg undo -k' to undo commits"
    )


@hint("amend-restack")
def hintrestack(node):
    return _(
        "descendants of %s are left behind - use 'hg restack' to rebase " "them"
    ) % short(node)


@hint("amend-autorebase")
def hintautorebase():
    return _(
        "descendants have been auto-rebased because no merge conflict "
        "could have happened - use --no-rebase or set "
        "commands.amend.autorebase=False to disable auto rebase"
    )


@hint("update-prev")
def hintprev():
    return _("use 'hg prev' to move to the parent changeset")


@hint("split-phabricator")
def hintsplitphabricator(advice):
    msg = _("some split commits have the same Phabricator Diff associated with them")
    if advice:
        msg += "\n" + advice
    return msg


def uisetup(ui):
    entry = extensions.wrapcommand(commands.table, "commit", commit)
    for opt in amendopts:
        opt = (opt[0], opt[1], opt[2], "(with --amend) " + opt[3])
        entry[1].append(opt)

    # manual call of the decorator
    command(
        "amend|am|ame|amen|ramen",
        [
            (
                "A",
                "addremove",
                None,
                _("mark new/missing files as added/removed before committing"),
            ),
            ("e", "edit", None, _("prompt to edit the commit message")),
            ("i", "interactive", None, _("use interactive mode")),
        ]
        + amendopts
        + commands.walkopts
        + commands.commitopts
        + commands.commitopts2,
        _("hg amend [OPTION]... [FILE]..."),
    )(amend)

    def has_automv(loaded):
        if not loaded:
            return
        automv = extensions.find("automv")
        entry = extensions.wrapcommand(cmdtable, "amend", automv.mvcheck)
        entry[1].append(
            ("", "no-move-detection", None, _("disable automatic file move detection"))
        )

    extensions.afterloaded("automv", has_automv)

    def evolveloaded(loaded):
        if not loaded:
            return

        evolvemod = extensions.find("evolve")

        # Remove conflicted commands from evolve.
        table = evolvemod.cmdtable
        for name in ["prev", "next", "split", "fold", "metaedit", "prune"]:
            todelete = [k for k in table if name in k]
            for k in todelete:
                oldentry = table[k]
                table["debugevolve%s" % name] = oldentry
                del table[k]

    extensions.afterloaded("evolve", evolveloaded)

    def rebaseloaded(loaded):
        if not loaded:
            return
        entry = extensions.wrapcommand(rebasemod.cmdtable, "rebase", wraprebase)
        entry[1].append(
            (
                "",
                "restack",
                False,
                _(
                    "rebase all changesets in the current "
                    "stack onto the latest version of their "
                    "respective parents"
                ),
            )
        )

    extensions.afterloaded("rebase", rebaseloaded)


def showtemplate(ui, repo, rev, **opts):
    if opts.get("template"):
        displayer = cmdutil.show_changeset(ui, repo, opts)
        displayer.show(rev)


def commit(orig, ui, repo, *pats, **opts):
    if opts.get("amend"):
        # commit --amend default behavior is to prompt for edit
        opts["noeditmessage"] = True
        return amend(ui, repo, *pats, **opts)
    else:
        badflags = [flag for flag in ["rebase", "fixup"] if opts.get(flag, None)]
        if badflags:
            raise error.Abort(_("--%s must be called with --amend") % badflags[0])

        rc = orig(ui, repo, *pats, **opts)
        current = repo["."]
        showtemplate(ui, repo, current, **opts)
        return rc


def amend(ui, repo, *pats, **opts):
    """save pending changes to the current commit

    Replaces your current commit with a new commit that contains the contents
    of the original commit, plus any pending changes.

    By default, all pending changes (in other words, those reported by 'hg
    status') are committed. To commit only some of your changes,
    you can:

    - Specify an exact list of files for which you want changes committed.

    - Use the -I or -X flags to pattern match file names to exclude or
      include by using a fileset. See 'hg help filesets' for more
      information.

    - Specify the --interactive flag to open a UI that will enable you
      to select individual insertions or deletions.

    By default, hg amend reuses your existing commit message and does not
    prompt you for changes. To change your commit message, you can:

    - Specify --edit / -e to open your configured editor to update the
      existing commit message.

    - Specify --message / -m to replace the entire commit message, including
      any commit template fields, with a string that you specify.

    .. note::

       Specifying -m overwrites all information in the commit message,
       including information specified as part of a pre-loaded commit
       template. For example, any information associating this commit with
       a code review system will be lost and might result in breakages.

    When you amend a commit that has descendants, those descendants are
    rebased on top of the amended version of the commit, unless doing so
    would result in merge conflicts. If this happens, run 'hg restack'
    to manually trigger the rebase so that you can go through the merge
    conflict resolution process.  You can also:

    - Specify --rebase to always trigger the rebase and resolve merge
      conflicts.

    - Specify --no-rebase to prevent the automatic rebasing of descendants.
    """
    # 'rebase' is a tristate option: None=auto, True=force, False=disable
    rebase = opts.get("rebase")
    to = opts.get("to")
    interactive = opts.get("interactive")

    if rebase and _histediting(repo):
        # if a histedit is in flight, it's dangerous to remove old commits
        hint = _("during histedit, use amend without --rebase")
        raise error.Abort("histedit in progress", hint=hint)

    badflags = [flag for flag in ["rebase", "fixup"] if opts.get(flag, None)]
    if interactive and badflags:
        raise error.Abort(
            _("--interactive and --%s are mutually exclusive") % badflags[0]
        )

    if interactive:
        with repo.wlock(), repo.lock():
            # Strip the interactive flag to avoid infinite recursive loop
            opts.pop("interactive")
            cmdutil.dorecord(
                ui, repo, amend, None, False, cmdutil.recordfilter, *pats, **opts
            )
            return

    fixup = opts.get("fixup")

    badtoflags = [
        "rebase",
        "fixup",
        "addremove",
        "edit",
        "message",
        "logfile",
        "date",
        "user",
        "no-move-detection",
        "stack",
        "template",
    ]

    badflags = [flag for flag in badtoflags if opts.get(flag, None)]
    if to and badflags:
        raise error.Abort(_(f"--to does not support --{badflags[0]}"))

    if fixup:
        ui.warn(
            _(
                "warning: --fixup is deprecated and WILL BE REMOVED. use 'hg restack' instead.\n"
            )
        )
        fixupamend(ui, repo)
        return

    if to:
        amendtocommit(ui, repo, to, pats, opts)
        return

    old = repo["."]
    if old.phase() == phases.public:
        raise error.Abort(_("cannot amend public changesets"))
    if len(repo[None].parents()) > 1:
        raise error.Abort(_("cannot amend while merging"))

    haschildren = bool(repo.revs("children(.)"))

    opts["message"] = cmdutil.logmessage(repo, opts)
    # Avoid further processing of any logfile. If such a file existed, its
    # contents have been copied into opts['message'] by logmessage
    opts["logfile"] = ""

    if not opts.get("noeditmessage") and not opts.get("message"):
        opts["message"] = old.description()

    commitdate = opts.get("date")
    if not commitdate:
        if ui.config("amend", "date") == "implicitupdate":
            commitdate = "now"
        else:
            commitdate = old.date()

    oldbookmarks = old.bookmarks()
    with repo.wlock(), repo.lock():
        node = cmdutil.amend(ui, repo, old, {}, pats, opts)

        if node == old.node():
            ui.status(_("nothing changed\n"))
            return 1

        conf = ui.config("amend", "autorestack", RESTACK_DEFAULT)
        noconflict = None

        # RESTACK_NO_CONFLICT requires IMM.
        if conf == RESTACK_NO_CONFLICT and not ui.config(
            "rebase", "experimental.inmemory", False
        ):
            conf = RESTACK_DEFAULT

        # If they explicitly disabled the old behavior, disable the new behavior
        # too, for now.
        # internal config: commands.amend.autorebase
        if ui.configbool("commands", "amend.autorebase") is False:
            # In the future we'll add a nag message here.
            conf = RESTACK_NEVER

        if conf not in RESTACK_VALUES:
            ui.warn(
                _('invalid amend.autorestack config of "%s"; falling back to %s\n')
                % (conf, RESTACK_DEFAULT)
            )
            conf = RESTACK_DEFAULT

        if haschildren and rebase is None and not _histediting(repo):
            if conf == RESTACK_ALWAYS:
                rebase = True
            elif conf == RESTACK_NO_CONFLICT:
                if repo[None].dirty():
                    # For now, only restack if the WC is clean (t31742174).
                    ui.status(_("not restacking because working copy is dirty\n"))
                    rebase = False
                else:
                    # internal config: amend.autorestackmsg
                    msg = ui.config(
                        "amend",
                        "autorestackmsg",
                        _("restacking children automatically (unless they conflict)"),
                    )
                    if msg:
                        ui.status("%s\n" % msg)
                    rebase = True
                    noconflict = True
            elif conf == RESTACK_ONLY_TRIVIAL:
                newcommit = repo[node]
                # If the rebase did not change the manifest and the
                # working copy is clean, force the children to be
                # restacked.
                rebase = (
                    old.manifestnode() == newcommit.manifestnode()
                    and not repo[None].dirty()
                )
                if rebase:
                    hintutil.trigger("amend-autorebase")
            else:
                rebase = False

        if haschildren and not rebase and not _histediting(repo):
            hintutil.trigger("amend-restack", old.node())

        changes = []
        # move old bookmarks to new node
        for bm in oldbookmarks:
            changes.append((bm, node))

        with repo.transaction("fixupamend") as tr:
            repo._bookmarks.applychanges(repo, tr, changes)

        if rebase and haschildren:
            noconflictmsg = _(
                "restacking would create conflicts (%s in %s), so you must run it manually\n(run `hg restack` manually to restack this commit's children)"
            )
            revs = [c.hex() for c in repo.set("(%n::)-%n", old.node(), old.node())]
            with ui.configoverride({("rebase", "noconflictmsg"): noconflictmsg}):
                # Note: this has effects on linearizing (old:: - old). That can
                # fail. If that fails, it might make sense to try a plain
                # rebase -s (old:: - old) -d new.
                restack.restack(ui, repo, rev=revs, noconflict=noconflict)

        showtemplate(ui, repo, repo[node], **opts)


def fixupamend(ui, repo, noconflict=None, noconflictmsg=None):
    """rebases any children found on the preamend changset and strips the
    preamend changset
    """
    wlock = None
    lock = None
    tr = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()
        current = repo["."]

        # Use obsolescence information to fix up the amend.
        common.restackonce(
            ui, repo, current.rev(), noconflict=noconflict, noconflictmsg=noconflictmsg
        )
    finally:
        lockmod.release(wlock, lock, tr)


def amendtocommit(ui, repo, commitspec, pats=None, opts=None):
    """amend to a specific commit

    This works by patching the working diff on to the specified commit
    and then performing a simplified rebase of the stack's tail on to
    the amended ancestor.

    commitspec must refer to a single commit that is a linear ancestor
    of ".".
    """
    with repo.wlock(), repo.lock(), repo.transaction("amend"):
        revs = list(scmutil.revrange(repo, [commitspec]))
        if len(revs) != 1:
            raise error.Abort(_("'%s' must refer to a single changeset") % commitspec)

        draftctxs = list(repo.revs("(%d)::.", revs[0]).iterctx())
        if len(draftctxs) == 0:
            raise error.Abort(
                _("revision '%s' is not an ancestor of the working copy") % commitspec
            )

        if repo.revs("%ld & merge()", draftctxs):
            raise error.Abort(_("cannot amend non-linear stack"))

        dest = draftctxs.pop(0)
        if dest.phase() == phases.public:
            raise error.Abort(_("cannot amend public changesets"))

        # Generate patch from wctx and apply to dest commit.
        mergedctx = mirrorwithmetadata(dest, dest.p1(), "amend")
        wctx = repo[None]
        matcher = scmutil.match(wctx, pats, opts) if pats or opts else None

        store = patch.mempatchstore(mergedctx)
        backend = patch.mempatchbackend(ui, mergedctx, store)
        ret = patch.applydiff(
            ui,
            io.BytesIO(b"".join(list(wctx.diff(match=matcher, opts=opts)))),
            backend,
            store,
        )
        if ret < 0:
            raise error.Abort(_("amend would conflict in %s") % ", ".join(backend.rejs))

        memctxs = [mergedctx]
        mappednodes = [dest.node()]

        # Perform mini-rebase of our stack.
        for ctx in draftctxs:
            memctxs.append(inmemorymerge(ui, repo, ctx, memctxs[-1], ctx.p1()))
            mappednodes.append(ctx.node())

        parentnode = None
        mapping = {}
        # Execute our list of in-memory commits, updating descendants'
        # parent as we go.
        for i, memctx in enumerate(memctxs):
            if i > 0:
                memctx = context.memctx.mirror(memctx, parents=[repo[parentnode]])
            parentnode = memctx.commit()
            mapping[mappednodes[i]] = (parentnode,)

        scmutil.cleanupnodes(repo, {dest.node(): mapping.pop(dest.node())}, "amend")
        scmutil.cleanupnodes(repo, mapping, "rebase")

        with repo.dirstate.parentchange():
            # Update dirstate status of amended files.
            repo.dirstate.rebuild(
                parentnode, repo[parentnode].manifest(), wctx.files(), exact=True
            )


def inmemorymerge(ui, repo, src, dest, base):
    """Return memctx representing three way merge of src, dest, and base

    src is "remote" and dest is "local".
    """
    mergeresult = nativecheckout.mergeresult(
        src.manifest(), dest.manifest(), base.manifest()
    )

    manifestbuilder = mergeresult.manifestbuilder()
    if manifestbuilder is None:
        raise error.Abort(
            _("amend would conflict in %s") % ", ".join(mergeresult.conflict_paths())
        )

    try:
        resolved = rebasemod._simplemerge(ui, base, src, dest, manifestbuilder)
    except error.InMemoryMergeConflictsError as ex:
        raise error.Abort(_("amend would conflict in %s") % ", ".join(ex.paths))

    mergedctx = mirrorwithmetadata(src, dest, "rebase")

    for path in manifestbuilder.removed():
        mergedctx[path] = None

    for path, merged in resolved.items():
        mergedctx[path] = context.overlayfilectx(
            src[path],
            datafunc=lambda data=merged: data,
            ctx=mergedctx,
        )

    return mergedctx


def mirrorwithmetadata(ctx, pctx, op):
    extra = ctx.extra().copy()
    extra[op + "_source"] = ctx.hex()
    mutinfo = mutation.record(ctx.repo(), extra, [ctx.node()], op)
    loginfo = {"predecessors": ctx.hex(), "mutation": op}
    return context.memctx.mirror(
        ctx, parents=[pctx], mutinfo=mutinfo, loginfo=loginfo, extra=extra
    )


def wraprebase(orig, ui, repo, *pats, **opts):
    """Wrapper around `hg rebase` adding the `--restack` option, which rebases
    all "unstable" descendants of an obsolete changeset onto the latest
    version of that changeset. This is similar to (and intended as a
    replacement for) the `hg evolve --all` command.
    """
    if opts["restack"]:
        # We can't abort if --dest is passed because some extensions
        # (namely remotenames) will automatically add this flag.
        # So just silently drop it instead.
        opts.pop("dest", None)

        if opts["rev"]:
            raise error.Abort(_("cannot use both --rev and --restack"))
        if opts["source"]:
            raise error.Abort(_("cannot use both --source and --restack"))
        if opts["base"]:
            raise error.Abort(_("cannot use both --base and --restack"))
        if opts["abort"]:
            raise error.Abort(_("cannot use both --abort and --restack"))
        if opts["continue"]:
            raise error.Abort(_("cannot use both --continue and --restack"))

        # The --hidden option is handled at a higher level, so instead of
        # checking for it directly we have to check whether the repo
        # is unfiltered.
        if repo.ui.configbool("visibility", "all-heads"):
            raise error.Abort(_("cannot use both --hidden and --restack"))

        return restack.restack(ui, repo, **opts)

    return orig(ui, repo, *pats, **opts)


def _histediting(repo):
    return repo.localvfs.exists("histedit-state")

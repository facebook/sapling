# prune.py - mark changesets as obsolete
#
# Copyright 2011 Peter Arrenbrecht <peter.arrenbrecht@gmail.com>
#                Logilab SA        <contact@logilab.fr>
#                Pierre-Yves David <pierre-yves.david@ens-lyon.org>
#                Patrick Mezard <patrick@mezard.eu>
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from edenscm.mercurial import (
    bookmarks as bookmarksmod,
    commands,
    error,
    extensions,
    hintutil,
    lock as lockmod,
    obsolete,
    registrar,
    repair,
    scmutil,
    util,
)
from edenscm.mercurial.i18n import _

from . import common


cmdtable = {}
command = registrar.command(cmdtable)


def _getmetadata(**opts):
    metadata = {}
    date = opts.get("date")
    user = opts.get("user")
    if date:
        metadata["date"] = "%i %i" % util.parsedate(date)
    if user:
        metadata["user"] = user
    return metadata


@command(
    "^prune",
    [
        ("s", "succ", [], _("successor changeset")),
        ("r", "rev", [], _("revisions to prune")),
        ("k", "keep", None, _("does not modify working copy during prune")),
        ("", "biject", False, _("do a 1-1 map between rev and successor ranges")),
        ("", "fold", False, _("record a fold (multiple precursors, one successors)")),
        ("", "split", False, _("record a split (on precursor, multiple successors)")),
        ("B", "bookmark", [], _("remove revs only reachable from given" " bookmark")),
        ("d", "date", "", _("record the specified date in metadata"), _("DATE")),
        ("u", "user", "", _("record the specified user in metadata"), _("USER")),
    ],
    _("[OPTION]... [[-r] REV]..."),
)
# XXX -U  --noupdate option to prevent wc update and or bookmarks update ?
def prune(ui, repo, *revs, **opts):
    """hide changesets by marking them obsolete

    Pruned changesets are obsolete with no successors. If they also have no
    descendants, they are hidden (invisible to all commands).

    Non-obsolete descendants of pruned changesets become "unstable". Use
    :hg:`evolve` to handle this situation.

    When you prune the parent of your working copy, Mercurial updates the
    working copy to a non-obsolete parent.

    You can use ``--succ`` to tell Mercurial that a newer version (successor)
    of the pruned changeset exists. Mercurial records successor revisions in
    obsolescence markers.

    You can use the ``--biject`` option to specify a 1-1 mapping (bijection)
    between revisions to pruned (precursor) and successor changesets. This
    option may be removed in a future release (with the functionality provided
    automatically).

    If you specify multiple revisions in ``--succ``, you are recording a
    "split" and must acknowledge it by passing ``--split``. Similarly, when you
    prune multiple changesets with a single successor, you must pass the
    ``--fold`` option.
    """
    if opts.get("keep", False):
        hint = "strip-uncommit"
    else:
        hint = "strip-hide"
    hintutil.trigger(hint)

    revs = scmutil.revrange(repo, list(revs) + opts.get("rev", []))
    succs = opts.get("succ", [])
    bookmarks = set(opts.get("bookmark", ()))
    metadata = _getmetadata(**opts)
    biject = opts.get("biject")
    fold = opts.get("fold")
    split = opts.get("split")

    options = [o for o in ("biject", "fold", "split") if opts.get(o)]
    if 1 < len(options):
        raise error.Abort(_("can only specify one of %s") % ", ".join(options))

    if bookmarks:
        revs += bookmarksmod.reachablerevs(repo, bookmarks)
        if not revs:
            # No revs are reachable exclusively from these bookmarks, just
            # delete the bookmarks.
            with repo.wlock(), repo.lock(), repo.transaction("prune-bookmarks") as tr:
                bookmarksmod.delete(repo, tr, bookmarks)
            for bookmark in sorted(bookmarks):
                ui.write(_("bookmark '%s' deleted\n") % bookmark)
            return 0

    if not revs:
        raise error.Abort(_("nothing to prune"))

    wlock = lock = tr = None
    try:
        wlock = repo.wlock()
        lock = repo.lock()
        tr = repo.transaction("prune")
        # defines pruned changesets
        precs = []
        revs.sort()
        for p in revs:
            cp = repo[p]
            if not cp.mutable():
                # note: createmarkers() would have raised something anyway
                raise error.Abort(
                    "cannot prune immutable changeset: %s" % cp,
                    hint="see 'hg help phases' for details",
                )
            precs.append(cp)
        if not precs:
            raise error.Abort("nothing to prune")

        # defines successors changesets
        sucs = scmutil.revrange(repo, succs)
        sucs.sort()
        sucs = tuple(repo[n] for n in sucs)
        if not biject and len(sucs) > 1 and len(precs) > 1:
            msg = "Can't use multiple successors for multiple precursors"
            hint = _("use --biject to mark a series as a replacement" " for another")
            raise error.Abort(msg, hint=hint)
        elif biject and len(sucs) != len(precs):
            msg = "Can't use %d successors for %d precursors" % (len(sucs), len(precs))
            raise error.Abort(msg)
        elif (len(precs) == 1 and len(sucs) > 1) and not split:
            msg = "please add --split if you want to do a split"
            raise error.Abort(msg)
        elif len(sucs) == 1 and len(precs) > 1 and not fold:
            msg = "please add --fold if you want to do a fold"
            raise error.Abort(msg)
        elif biject:
            relations = [(p, (s,)) for p, s in zip(precs, sucs)]
        else:
            relations = [(p, sucs) for p in precs]

        wdp = repo["."]

        if len(sucs) == 1 and len(precs) == 1 and wdp in precs:
            # '.' killed, so update to the successor
            newnode = sucs[0]
        else:
            # update to an unkilled parent
            newnode = wdp

            while newnode in precs or newnode.obsolete():
                newnode = newnode.parents()[0]

        if newnode.node() != wdp.node():
            if opts.get("keep", False):
                # This is largely the same as the implementation in
                # strip.stripcmd(). We might want to refactor this somewhere
                # common at some point.

                # only reset the dirstate for files that would actually change
                # between the working context and uctx
                descendantrevs = repo.revs("%d::." % newnode.rev())
                changedfiles = []
                for rev in descendantrevs:
                    # blindly reset the files, regardless of what actually
                    # changed
                    changedfiles.extend(repo[rev].files())

                # reset files that only changed in the dirstate too
                dirstate = repo.dirstate
                dirchanges = [f for f in dirstate if dirstate[f] != "n"]
                changedfiles.extend(dirchanges)
                repo.dirstate.rebuild(newnode.node(), newnode.manifest(), changedfiles)
                dirstate.write(tr)
            else:
                bookactive = repo._activebookmark
                # Active bookmark that we don't want to delete (with -B option)
                # we deactivate and move it before the update and reactivate it
                # after
                movebookmark = bookactive and not bookmarks
                if movebookmark:
                    bookmarksmod.deactivate(repo)
                    changes = [(bookactive, newnode.node())]
                    repo._bookmarks.applychanges(repo, tr, changes)
                commands.update(ui, repo, newnode.rev())
                ui.status(
                    _("working directory now at %s\n")
                    % ui.label(str(newnode), "evolve.node")
                )
                if movebookmark:
                    bookmarksmod.activate(repo, bookactive)

        # update bookmarks
        if bookmarks:
            with repo.wlock(), repo.lock(), repo.transaction("prune-bookmarks") as tr:
                bookmarksmod.delete(repo, tr, bookmarks)
            for bookmark in sorted(bookmarks):
                ui.write(_("bookmark '%s' deleted\n") % bookmark)

        # create markers
        obsolete.createmarkers(repo, relations, metadata=metadata, operation="prune")

        # informs that changeset have been pruned
        ui.status(_("%i changesets pruned\n") % len(precs))

        for ctx in repo.unfiltered().set("bookmark() and %ld", precs):
            # used to be:
            #
            #   ldest = list(repo.set('max((::%d) - obsolete())', ctx))
            #   if ldest:
            #      c = ldest[0]
            #
            # but then revset took a lazy arrow in the knee and became much
            # slower. The new forms makes as much sense and a much faster.
            for dest in ctx.ancestors():
                if not dest.obsolete():
                    updatebookmarks = common.bookmarksupdater(repo, ctx.node(), tr)
                    updatebookmarks(dest.node())
                    break

        tr.close()
    finally:
        lockmod.release(tr, lock, wlock)

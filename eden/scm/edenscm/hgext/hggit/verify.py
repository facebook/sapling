# verify.py - verify Mercurial revisions
#
# Copyright 2014 Facebook.
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.

import stat

from dulwich import diff_tree
from dulwich.objects import S_IFGITLINK, Commit
from edenscm.mercurial import error, progress, util as hgutil
from edenscm.mercurial.i18n import _


def verify(ui, repo, hgctx):
    """verify that a Mercurial rev matches the corresponding Git rev

    Given a Mercurial revision that has a corresponding Git revision in the map,
    this attempts to answer whether that revision has the same contents as the
    corresponding Git revision.

    """
    handler = repo.githandler

    gitsha = handler.map_git_get(hgctx.hex())
    if not gitsha:
        # TODO deal better with commits in the middle of octopus merges
        raise hgutil.Abort(
            _("no git commit found for rev %s") % hgctx,
            hint=_("if this is an octopus merge, " "verify against the last rev"),
        )

    try:
        gitcommit = handler.git.get_object(gitsha)
    except KeyError:
        raise hgutil.Abort(
            _("git equivalent %s for rev %s not found!") % (gitsha, hgctx)
        )
    if not isinstance(gitcommit, Commit):
        raise hgutil.Abort(
            _("git equivalent %s for rev %s is not a commit!") % (gitsha, hgctx)
        )

    ui.status(_("verifying rev %s against git commit %s\n") % (hgctx, gitsha))
    failed = False

    # TODO check commit message and other metadata

    dirkind = stat.S_IFDIR

    hgfiles = set(hgctx)
    gitfiles = set()

    i = 0
    with progress.bar(ui, _("verify"), total=len(hgfiles)) as prog:
        for gitfile, dummy in diff_tree.walk_trees(
            handler.git.object_store, gitcommit.tree, None
        ):
            if gitfile.mode == dirkind:
                continue
            # TODO deal with submodules
            if gitfile.mode == S_IFGITLINK:
                continue
            prog.value = i
            i += 1
            gitfiles.add(gitfile.path)

            try:
                fctx = hgctx[gitfile.path]
            except error.LookupError:
                # we'll deal with this at the end
                continue

            hgflags = fctx.flags()
            gitflags = handler.convert_git_int_mode(gitfile.mode)
            if hgflags != gitflags:
                ui.write(
                    _("file has different flags: %s (hg '%s', git '%s')\n")
                    % (gitfile.path, hgflags, gitflags)
                )
                failed = True
            if fctx.data() != handler.git[gitfile.sha].data:
                ui.write(_("difference in: %s\n") % gitfile.path)
                failed = True

    if hgfiles != gitfiles:
        failed = True
        missing = gitfiles - hgfiles
        for f in sorted(missing):
            ui.write(_("file found in git but not hg: %s\n") % f)
        unexpected = hgfiles - gitfiles
        for f in sorted(unexpected):
            ui.write(_("file found in hg but not git: %s\n") % f)

    if failed:
        return 1
    else:
        return 0

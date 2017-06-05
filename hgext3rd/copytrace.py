# Copyright 2017-present Facebook. All Rights Reserved.
#
# faster copytrace implementation
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

'''extension that does copytracing fast

::

    [copytrace]
    # whether to enable fast copytracing or not
    fastcopytrace = False

    # limits the number of commits in the source "branch" i. e. "branch".
    # that is rebased or merged. These are the commits from base up to csrc
    # (see _mergecopies docblock below).
    # copytracing can be too slow if there are too
    # many commits in this "branch".
    sourcecommitlimit = 100

    # limits the number of heuristically found move candidates to check
    maxmovescandidatestocheck = 5
'''

from collections import defaultdict
from mercurial import (
    commands,
    copies as copiesmod,
    dispatch,
    extensions,
    filemerge,
    util,
)
from mercurial.i18n import _

import os
import time

_copytracinghint = ("hint: if this message is due to a moved file, you can " +
                    "ask mercurial to attempt to automatically resolve this " +
                    "change by re-running with the --tracecopies flag, but " +
                    "this will significantly slow down the operation, so you " +
                    "will need to be patient.\n" +
                    "Source control team is working on fixing this problem.\n")

def uisetup(ui):
    extensions.wrapfunction(dispatch, "runcommand", _runcommand)

def extsetup(ui):
    commands.globalopts.append(
        ("", "tracecopies", None,
         _("enable copytracing. Warning: can be very slow!")))

    # With experimental.disablecopytrace=True there can be cryptic merge errors.
    # Let"s change error message to suggest re-running the command with
    # enabled copytracing
    filemerge._localchangedotherdeletedmsg = _(
        "local%(l)s changed %(fd)s which other%(o)s deleted\n" +
        _copytracinghint +
        "use (c)hanged version, (d)elete, or leave (u)nresolved?"
        "$$ &Changed $$ &Delete $$ &Unresolved")

    filemerge._otherchangedlocaldeletedmsg = _(
        "other%(o)s changed %(fd)s which local%(l)s deleted\n" +
        _copytracinghint +
        "use (c)hanged version, leave (d)eleted, or leave (u)nresolved?"
        "$$ &Changed $$ &Deleted $$ &Unresolved")

    extensions.wrapfunction(filemerge, '_filemerge', _filemerge)
    extensions.wrapfunction(copiesmod, 'mergecopies', _mergecopies)

def _filemerge(origfunc, premerge, repo, mynode, orig, fcd, fco, fca,
               labels=None, *args, **kwargs):
    if premerge:
        if orig != fco.path():
            # copytracing was in action, let's record it
            if not repo.ui.configbool("experimental", "disablecopytrace"):
                msg = "success"
            else:
                msg = "success (fastcopytracing)"
            repo.ui.log("copytrace", msg=msg,
                        reponame=_getreponame(repo, repo.ui))
    return origfunc(premerge, repo, mynode, orig, fcd, fco, fca, labels,
                *args, **kwargs)

def _runcommand(orig, lui, repo, cmd, fullargs, ui, *args, **kwargs):
    if "--tracecopies" in fullargs:
        ui.setconfig("experimental", "disablecopytrace",
                     False, "--tracecopies")
    return orig(lui, repo, cmd, fullargs, ui, *args, **kwargs)

def _mergecopies(orig, repo, cdst, csrc, base):
    start = time.time()
    try:
        return _domergecopies(orig, repo, cdst, csrc, base)
    except Exception as e:
        # make sure we don't break clients
        repo.ui.log("copytrace", "Copytrace failed: %s" % e,
                    reponame=_getreponame(repo, repo.ui))
        return {}, {}, {}, {}, {}
    finally:
        repo.ui.log("copytracingduration", "",
                    copytracingduration=time.time() - start,
                    fastcopytraceenabled=_fastcopytraceenabled(repo.ui))

def _domergecopies(orig, repo, cdst, csrc, base):
    """ Fast copytracing using filename heuristics

    Handle one case where we assume there are no merge commits in
    "source branch". Source branch is commits from base up to csrc not
    including base.
    If these assumptions don't hold then we fallback to the
    upstream mergecopies

    p
    |
    p  <- cdst - rebase or merge destination, can be draft
    .
    .
    .   d  <- csrc - commit to be rebased or merged.
    |   |
    p   d  <- base
    | /
    p  <- common ancestor

    To find copies we are looking for files with similar filenames.
    See description of the heuristics below.

    Upstream copytracing function returns five dicts:
    "copy", "movewithdir", "diverge", "renamedelete" and "dirmove". See below
    for a more detailed description (mostly copied from upstream).
    This extension returns "copy" dict only, everything else is empty.

    "copy" is a mapping from destination name -> source name,
    where source is in csrc and destination is in cdst or vice-versa.

    "movewithdir" is a mapping from source name -> destination name,
    where the file at source present in one context but not the other
    needs to be moved to destination by the merge process, because the
    other context moved the directory it is in.

    "diverge" is a mapping of source name -> list of destination names
    for divergent renames. On the time of writing this extension it was used
    only to print warning.

    "renamedelete" is a mapping of source name -> list of destination
    names for files deleted in c1 that were renamed in c2 or vice-versa.
    On the time of writing this extension it was used only to print warning.

    "dirmove" is a mapping of detected source dir -> destination dir renames.
    This is needed for handling changes to new files previously grafted into
    renamed directories.

    """

    if not repo.ui.configbool("experimental", "disablecopytrace"):
        # user explicitly enabled copytracing - use it
        return orig(repo, cdst, csrc, base)

    if not _fastcopytraceenabled(repo.ui):
        return orig(repo, cdst, csrc, base)

    if not cdst or not csrc or cdst == csrc:
        return {}, {}, {}, {}, {}

    # avoid silly behavior for parent -> working dir
    if csrc.node() is None and cdst.node() == repo.dirstate.p1():
        return repo.dirstate.copies(), {}, {}, {}, {}

    if cdst.rev() is None:
        cdst = cdst.p1()
    if csrc.rev() is None:
        csrc = csrc.p1()

    copies = {}

    ctx = csrc
    changedfiles = set()
    sourcecommitnum = 0
    sourcecommitlimit = repo.ui.configint('copytrace', 'sourcecommitlimit', 100)
    mdst = cdst.manifest()
    while ctx != base:
        if len(ctx.parents()) == 2:
            # To keep things simple let's not handle merges
            return orig(repo, cdst, csrc, base)
        changedfiles.update(ctx.files())
        ctx = ctx.p1()
        sourcecommitnum += 1
        if sourcecommitnum > sourcecommitlimit:
            return orig(repo, cdst, csrc, base)

    cp = copiesmod._forwardcopies(base, csrc)
    for dst, src in cp.iteritems():
        if src in mdst:
            copies[dst] = src

    missingfiles = filter(lambda f: f not in mdst, changedfiles)
    if missingfiles:
        # Use the following file name heuristic to find moves: moves are
        # usually either directory moves or renames of the files in the
        # same directory. That means that we can look for the files in dstc
        # with either the same basename or the same dirname.
        basenametofilename = defaultdict(list)
        dirnametofilename = defaultdict(list)
        for f in mdst.filesnotin(base.manifest()):
            basename = os.path.basename(f)
            dirname = os.path.dirname(f)
            basenametofilename[basename].append(f)
            dirnametofilename[dirname].append(f)

        maxmovecandidatestocheck = repo.ui.configint(
            'copytrace', 'maxmovescandidatestocheck', 5)
        # in case of a rebase/graft, base may not be a common ancestor
        anc = cdst.ancestor(csrc)
        for f in missingfiles:
            basename = os.path.basename(f)
            dirname = os.path.dirname(f)
            samebasename = basenametofilename[basename]
            samedirname = dirnametofilename[dirname]
            movecandidates = samebasename + samedirname
            # if file "f" is not present in csrc that means that it was deleted
            # in cdst and csrc. Ignore "f" in that case
            if f in csrc:
                f2 = csrc.filectx(f)
                for candidate in movecandidates[:maxmovecandidatestocheck]:
                    f1 = cdst.filectx(candidate)
                    if copiesmod._related(f1, f2, anc.rev()):
                        # if there are a few related copies then we'll merge
                        # changes into all of them. This matches the behaviour
                        # of upstream copytracing
                        copies[candidate] = f
                if len(movecandidates) > maxmovecandidatestocheck:
                    msg = "too many moves candidates: %d" % len(movecandidates)
                    repo.ui.log("copytrace", msg=msg,
                                reponame=_getreponame(repo, repo.ui))

    return copies, {}, {}, {}, {}

def _fastcopytraceenabled(ui):
    return ui.configbool("copytrace", "fastcopytrace", False)

def _getreponame(repo, ui):
    reporoot = repo.origroot if util.safehasattr(repo, 'origroot') else ''
    reponame = ui.config('paths', 'default', reporoot)
    if reponame:
        reponame = os.path.basename(reponame)
    return reponame

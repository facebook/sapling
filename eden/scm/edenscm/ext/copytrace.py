# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""extension that does copytracing fast

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

    # whether to enable fast copytracing during amends (requires fastcopytrace
    # to be enabled.)
    enableamendcopytrace = True

    # how many previous commits to search through when looking for amend
    # copytrace data.
    amendcopytracecommitlimit = 100

    # whether to enable full copytracing on small draft branches.
    # Disabled by default
    draftusefullcopytrace = False

"""

import codecs
import collections
import os
import sys
import time

from edenscm import (
    cmdutil,
    copies as copiesmod,
    dispatch,
    extensions,
    filemerge,
    json,
    node,
    phases,
    pycompat,
    registrar,
    scmutil,
    util,
)
from edenscm.i18n import _


if sys.version_info[0] < 3:
    import anydbm as dbm
else:
    import dbm


configtable = {}
configitem = registrar.configitem(configtable)

configitem("copytrace", "maxmovescandidatestocheck", default=5)
configitem("copytrace", "sourcecommitlimit", default=100)
configitem("copytrace", "fastcopytrace", default=False)
configitem("copytrace", "enableamendcopytrace", default=True)
configitem("copytrace", "amendcopytracecommitlimit", default=100)

defaultdict = collections.defaultdict
_copytracinghint = (
    "hint: if this message is due to a moved file, you can "
    + "ask mercurial to attempt to automatically resolve this "
    + "change by re-running with the --config=experimental.copytrace=on flag, but "
    + "this will significantly slow down the operation, so you "
    + "will need to be patient.\n"
    + "Source control team is working on fixing this problem.\n"
)


def uisetup(ui):
    extensions.wrapfunction(dispatch, "runcommand", _runcommand)


def extsetup(ui):
    # With experimental.copytrace=off there can be cryptic merge errors.
    # Let's change error message to suggest re-running the command with
    # enabled copytracing
    filemerge._localchangedotherdeletedmsg = _(
        "local%(l)s changed %(fd)s which other%(o)s deleted%(fa)s\n"
        + _copytracinghint
        + "use (c)hanged version, (d)elete, or leave (u)nresolved?"
        "$$ &Changed $$ &Delete $$ &Unresolved"
    )

    filemerge._otherchangedlocaldeletedmsg = _(
        "other%(o)s changed %(fd)s which local%(l)s deleted\n"
        "hint: if this is due to a renamed file, you can manually input the renamed path, or "
        "re-run the command using --config=experimental.copytrace=on to make @prog@ figure out "
        "renamed path automatically (which is very slow, and you will need to be patient)\n"
        "use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path?"
        "$$ &Changed $$ &Deleted $$ &Unresolved $$ &Renamed"
    )

    extensions.wrapfunction(filemerge, "_filemerge", _filemerge)
    extensions.wrapfunction(copiesmod, "mergecopies", _mergecopies)
    extensions.wrapfunction(cmdutil, "amend", _amend)


def _filemerge(
    origfunc,
    premerge,
    repo,
    wctx,
    mynode,
    orig,
    fcd,
    fco,
    fca,
    labels=None,
    *args,
    **kwargs,
):

    if premerge:
        # copytracing worked if files to merge have different file names
        # and filelog contents are different (fco.cmp(fcd) returns True if
        # they are different). If filelog contents are the same then the file
        # was moved in the rebase/graft/merge source, but wasn't changed in the
        # rebase/graft/merge destination. This case mercurial would've handled
        # even with disabled copytracing, so we don't want to log it.
        if orig != fco.path() and fco.cmp(fcd):
            # copytracing was in action, let's record it
            if repo.ui.config("experimental", "copytrace") == "on":
                msg = "success (fastcopytracing)"
            else:
                msg = "success"

            try:
                destctx = _getctxfromfctx(fcd)
                srcctx = _getctxfromfctx(fco)
                hexes = "%s, %s" % (_gethex(destctx), _gethex(srcctx))
                paths = "%s, %s" % (orig, fco.path())
                msg = "%s (%s; %s)" % (msg, hexes, paths)
            except Exception as e:
                # we don't expect any exceptions to happen, but to be 100%
                # sure we don't break hg let's catch everything and log it
                msg = "failed to log: %s" % (e,)
            repo.ui.log("copytrace", msg=msg, reponame=_getreponame(repo, repo.ui))

    return origfunc(
        premerge, repo, wctx, mynode, orig, fcd, fco, fca, labels, *args, **kwargs
    )


def _runcommand(orig, lui, repo, cmd, fullargs, ui, *args, **kwargs):
    return orig(lui, repo, cmd, fullargs, ui, *args, **kwargs)


def opendbm(repo, flag):
    """Open the dbm of choice.

    On some platforms, dbm is available, on others it's not,
    but gdbm is unfortunately not available everywhere, like on Windows.
    """
    dbms = [(dbm.open, "amendcopytrace", dbm.error)]

    for opener, fname, error in dbms:
        path = repo.localvfs.join(fname)
        try:
            return (opener(path, flag), error)
        except error:
            continue
        except ImportError:
            continue

    return None, None


def _amend(orig, ui, repo, old, extra, pats, opts):
    """Wraps amend to collect copytrace data on amend

    If a file is created in one commit, modified in a subsequent commit, and
    then renamed or copied by amending the original commit, restacking the
    commits that modify the file will fail:

    file modified here    B     B'  restack of B to B' will fail
                          |     :
    file created here     A --> A'  file renamed in amended commit
                          |    /
                          o --

    This function collects information about copies and renames from amend
    commits, and saves it for use during rebases onto the amend commit.  This
    lets rebases onto files that been renamed or copied in an amend commit
    work without conflicts.

    This function collects the copytrace information from the working copy and
    stores it against the amended commit in a separate dbm file. Later,
    in _domergecopies, this information will be merged with the rebase
    copytrace data to incorporate renames and copies made during the amend.
    """

    # Check if amend copytracing has been disabled.
    if not ui.configbool("copytrace", "enableamendcopytrace"):
        return orig(ui, repo, old, extra, pats, opts)

    # Need to get the amend-copies before calling the command because files from
    # the working copy will be used during the amend.
    wctx = repo[None]

    # Find the amend-copies.
    matcher = scmutil.match(wctx, pats, opts)
    amend_copies = copiesmod.pathcopies(old, wctx, matcher)

    # Finally, invoke the command.
    node = orig(ui, repo, old, extra, pats, opts)
    amended_ctx = repo[node]

    # Store the amend-copies against the amended context.
    if amend_copies:
        db, error = opendbm(repo, "c")
        if db is None:
            # Database locked, can't record these amend-copies.
            ui.log("copytrace", "Failed to open amendcopytrace db: %s" % error)
            return node

        # Merge in any existing amend copies from any previous amends.
        try:
            orig_data = db[old.node()]
        except KeyError:
            orig_data = "{}"
        except error as e:
            ui.log(
                "copytrace",
                "Failed to read key %s from amendcopytrace db: %s" % (old.hex(), e),
            )
            return node

        orig_encoded = json.loads(orig_data)
        orig_amend_copies = dict(
            (
                pycompat.decodeutf8(codecs.decode(pycompat.encodeutf8(k), "base64")),
                pycompat.decodeutf8(codecs.decode(pycompat.encodeutf8(v), "base64")),
            )
            for (k, v) in pycompat.iteritems(orig_encoded)
        )

        # Copytrace information is not valid if it refers to a file that
        # doesn't exist in a commit.  We need to update or remove entries
        # that refer to files that might have only existed in the previous
        # amend commit.
        #
        # Find chained copies and renames (a -> b -> c) and collapse them to
        # (a -> c).  Delete the entry for b if this was a rename.
        for dst, src in pycompat.iteritems(amend_copies):
            if src in orig_amend_copies:
                amend_copies[dst] = orig_amend_copies[src]
                if src not in amended_ctx:
                    del orig_amend_copies[src]

        # Copy any left over copies from the previous context.
        for dst, src in pycompat.iteritems(orig_amend_copies):
            if dst not in amend_copies:
                amend_copies[dst] = src

        # Write out the entry for the new amend commit.
        encoded = dict(
            (
                pycompat.decodeutf8(codecs.encode(pycompat.encodeutf8(k), "base64")),
                pycompat.decodeutf8(codecs.encode(pycompat.encodeutf8(v), "base64")),
            )
            for (k, v) in pycompat.iteritems(amend_copies)
        )
        db[node] = json.dumps(encoded)
        try:
            db.close()
        except Exception as e:
            # Database corruption.  Not much we can do, so just log.
            ui.log("copytrace", "Failed to close amendcopytrace db: %s" % e)

    return node


def _getamendcopies(repo, dest, ancestor):
    db, error = opendbm(repo, "r")
    if db is None:
        return {}
    try:
        ctx = dest
        count = 0
        limit = repo.ui.configint("copytrace", "amendcopytracecommitlimit")

        # Search for the ancestor commit that has amend copytrace data.  This
        # will be the most recent amend commit if we are rebasing onto an
        # amend commit.  If we reach the common ancestor or a public commit,
        # then there is no amend copytrace data to be found.
        while ctx.node() not in db:
            ctx = ctx.p1()
            count += 1
            if ctx == ancestor or count > limit or ctx.phase() == phases.public:
                return {}

        # Load the amend copytrace data from this commit.
        encoded = json.loads(db[ctx.node()])
        return dict(
            (
                codecs.decode(k.encode("utf8"), "base64").decode("utf8"),
                codecs.decode(v.encode("utf8"), "base64").decode("utf8"),
            )
            for (k, v) in pycompat.iteritems(encoded)
        )
    except Exception:
        repo.ui.log("copytrace", "Failed to load amend copytrace for %s" % dest.hex())
        return {}
    finally:
        try:
            db.close()
        except error:
            pass


def _mergecopies(orig, repo, cdst, csrc, base):
    start = time.time()
    try:
        return _domergecopies(orig, repo, cdst, csrc, base)
    except Exception as e:
        # make sure we don't break clients
        repo.ui.log(
            "copytrace",
            "Copytrace failed: %s" % e,
            reponame=_getreponame(repo, repo.ui),
        )
        return {}, {}, {}, {}, {}
    finally:
        repo.ui.log(
            "copytracingduration",
            "",
            copytracingduration=time.time() - start,
            fastcopytraceenabled=_fastcopytraceenabled(repo.ui),
        )


@util.timefunction("mergecopies")
def _domergecopies(orig, repo, cdst, csrc, base):
    """Fast copytracing using filename heuristics

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
    .   d  <- csrc - commit to be rebased or merged or grafted.
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

    if repo.ui.config("experimental", "copytrace") == "on":
        # user explicitly enabled copytracing - use it
        return orig(repo, cdst, csrc, base)

    if not _fastcopytraceenabled(repo.ui):
        return orig(repo, cdst, csrc, base)

    # If base, source and destination are all draft branches, let's use full
    # copytrace for increased capabilities since it will work fast enough
    if _isfullcopytraceable(repo.ui, cdst, base):
        configoverrides = {("experimental", "copytrace"): "on"}
        with repo.ui.configoverride(configoverrides, "mergecopies"):
            result = orig(repo, cdst, csrc, base)
            if repo.ui.configbool("copytrace", "enableamendcopytrace"):
                # Look for additional amend-copies
                amend_copies = _getamendcopies(repo, cdst, base.p1())
                # update result[0] dict w/ amend_copies
                result[0].update(amend_copies)
                result[0] = _filtercopies(result[0], cdst, csrc, base)

        return result

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
    sourcecommitlimit = repo.ui.configint("copytrace", "sourcecommitlimit")
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
    for dst, src in pycompat.iteritems(cp):
        if src in mdst:
            copies[dst] = src

    # file is missing if it isn't present in the destination, but is present in
    # the base and present in the source.
    # Presence in the base is important to exclude added files, presence in the
    # source is important to exclude removed files.
    missingfiles = list(
        filter(lambda f: f not in mdst and f in base and f in csrc, changedfiles)
    )
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
            "copytrace", "maxmovescandidatestocheck"
        )
        # in case of a rebase/graft, base may not be a common ancestor
        anc = cdst.ancestor(csrc)
        for f in missingfiles:
            basename = os.path.basename(f)
            dirname = os.path.dirname(f)
            samebasename = basenametofilename[basename]
            samedirname = dirnametofilename[dirname]
            movecandidates = samebasename + samedirname
            # f is guaranteed to be present in csrc, that's why
            # csrc.filectx(f) won't fail
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
                repo.ui.log("copytrace", msg=msg, reponame=_getreponame(repo, repo.ui))

    if repo.ui.configbool("copytrace", "enableamendcopytrace"):
        # Look for additional amend-copies.
        amend_copies = _getamendcopies(repo, cdst, base.p1())
        if amend_copies:
            repo.ui.debug("Loaded amend copytrace for %s" % cdst)
            for dst, src in pycompat.iteritems(amend_copies):
                if dst not in copies:
                    copies[dst] = src

    return _filtercopies(copies, cdst, csrc, base), {}, {}, {}, {}


def _filtercopies(copies, cdst, csrc, base):
    """Remove uninteresting copies if files are not changed.

    The mergecopies function is expected to report cases where one side renames
    a file, while the other side changed the file before the rename.

    In case there is only renaming without changing, do not report the copy.
    In fact, reporting the copy can confuse other part of merge.py and cause
    files to be deleted incorrectly.

    This post-processing is currently known only necessary to the heuristics
    algorithm, but not necessary for the original, slow "full copytracing" code
    path.
    """
    newcopies = {}
    if copies:
        # Warm-up manifests
        cdst.manifest()
        csrc.manifest()
        base.manifest()
        for fdst, fsrc in copies.items():
            if fsrc not in base:
                # Should not happen. Just be graceful in case something went
                # wrong.
                continue
            basenode = base[fsrc].filenode()
            if fsrc in cdst and cdst[fsrc].filenode() == basenode:
                continue
            if fsrc in csrc and csrc[fsrc].filenode() == basenode:
                continue
            newcopies[fdst] = fsrc
    return newcopies


def _fastcopytraceenabled(ui):
    return ui.configbool("copytrace", "fastcopytrace")


def _getreponame(repo, ui):
    reporoot = repo.origroot if util.safehasattr(repo, "origroot") else ""
    reponame = ui.config("paths", "default") or reporoot
    if reponame:
        reponame = os.path.basename(reponame)
    return reponame


def _getctxfromfctx(fctx):
    if fctx.isabsent():
        return fctx._ctx
    else:
        return fctx._changectx


def _gethex(ctx):
    # for workingctx return p1 hex
    return ctx.hex() if ctx.hex() != node.wdirhex else ctx.p1().hex()


def _isfullcopytraceable(ui, cdst, base):
    if not ui.configbool("copytrace", "draftusefullcopytrace", False):
        return False
    if cdst.phase() == phases.draft and base.phase() == phases.draft:
        # draft branch: Use traditional copytracing if < 100 commits
        ctx = cdst
        commits = 0
        sourcecommitlimit = ui.configint("copytrace", "sourcecommitlimit")
        while ctx != base and commits != sourcecommitlimit:
            ctx = ctx.p1()
            commits += 1
        return commits < sourcecommitlimit

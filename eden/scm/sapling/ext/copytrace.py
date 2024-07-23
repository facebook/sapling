# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""extension that does copytracing fast

Copy tracing is mainly used for automatically detecting renames in @Product@ commands like
`rebase`, `graft`, `amend` etc. For example, assuming we have a commit graph like below:

::

    D        # updates b.txt
    |
    C        # moves a.txt -> b.txt
    |
    | B      # updates a.txt
    |/
    A        # merge base

When we try to rebase commit `B` onto commit `D`, copy tracing will automatically
detect that `a.txt` was renamed io `b.txt` in commit `C` and `b.txt` exists in commit `D`,
so @Product@ will merge `a.txt` of commit `B` into `b.txt` of commit `D` instead of
prompting a message saying 'a.txt' is not in commit `D` and ask user to resolve the
conflict.

The copy tracing algorithm supports both @Product@ and Git format repositories, the difference
between them are:

::

    - @Product@ format: the rename information is stored in file's header.
    - Git format: there is no rename information stored in the repository, we
    need to compute a content-similarity-score for two files, if the similarity score is higher
    than a threshold, we treat them as a rename.


The following are configs to tune the behavior of copy tracing algorithm:

::

    [copytrace]
    # Whether to fallback to content similarity rename detection. This is used for
    # @Product@ format repositories in case users forgot to record rename information
    # with `@prog@ mv`.
    fallback-to-content-similarity = True

    # Maximum rename edit (`add`, `delete`) cost, if the edit cost of two files exceeds this
    # threshold, we will not treat them as a rename no matter what the content similarity is.
    max-edit-cost = 1000

    # Content similarity threhold for rename detection. The definition of "similarity"
    # between file `a` and file `b` is: (len(a.lines()) - edit_cost(a, b)) / len(a.lines())
    #   * 1.0 means exact match
    #   * 0.0 means not match at all
    similarity-threshold = 0.8

    # limits the number of commits in the source "branch" i. e. "branch".
    # that is rebased or merged. These are the commits from base up to csrc
    # (see _mergecopies docblock below).
    # copytracing can be too slow if there are too
    # many commits in this "branch".
    sourcecommitlimit = 100

    # whether to enable fast copytracing during amends
    enableamendcopytrace = True

    # how many previous commits to search through when looking for amend
    # copytrace data.
    amendcopytracecommitlimit = 100
"""

from sapling import copies as copiesmod, extensions, registrar, util
from sapling.i18n import _


configtable = {}
configitem = registrar.configitem(configtable)

configitem("copytrace", "sourcecommitlimit", default=100)
configitem("copytrace", "enableamendcopytrace", default=True)
configitem("copytrace", "amendcopytracecommitlimit", default=100)
configitem("copytrace", "dagcopytrace", default=False)


def extsetup(ui) -> None:
    extensions.wrapfunction(copiesmod, "mergecopies", _mergecopies)


@util.timefunction("mergecopies")
def _mergecopies(orig, repo, cdst, csrc, base):
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

    Return a mapping from destination name -> source name,
    where source is in csrc and destination is in cdst or vice-versa.

    """

    # todo: make copy tracing support directory move detection

    # avoid silly behavior for parent -> working dir
    if csrc.node() is None and cdst.node() == repo.dirstate.p1():
        return repo.dirstate.copies()

    orig_cdst = cdst
    if cdst.rev() is None:
        cdst = cdst.p1()
    if csrc.rev() is None:
        csrc = csrc.p1()

    copies = {}

    changedfiles = set()
    sourcecommitnum = 0
    sourcecommitlimit = repo.ui.configint("copytrace", "sourcecommitlimit")
    mdst = cdst.manifest()

    if repo.ui.cmdname == "backout":
        # for `backout` operation, `base` is the commit we want to backout and
        # `csrc` is the parent of the `base` commit.
        curr, target = base, csrc
    else:
        # for normal cases, `base` is the parent of `csrc`
        curr, target = csrc, base

    while curr != target:
        if len(curr.parents()) == 2:
            # To keep things simple let's not handle merges
            return orig(repo, cdst, csrc, base)
        changedfiles.update(curr.files())
        curr = curr.p1()
        sourcecommitnum += 1
        if sourcecommitnum > sourcecommitlimit:
            return orig(repo, cdst, csrc, base)

    cp = copiesmod.pathcopies(base, csrc)
    for dst, src in _filtercopies(cp, base, cdst).items():
        if src in orig_cdst or dst in orig_cdst:
            copies[dst] = src

    # file is missing if it isn't present in the destination, but is present in
    # the base and present in the source.
    # Presence in the base is important to exclude added files, presence in the
    # source is important to exclude removed files.
    missingfiles = list(
        filter(lambda f: f not in mdst and f in base and f in csrc, changedfiles)
    )
    repo.ui.metrics.gauge("copytrace_missingfiles", len(missingfiles))
    if missingfiles and _dagcopytraceenabled(repo.ui):
        dag_copy_trace = repo._dagcopytrace
        dst_copies = dag_copy_trace.trace_renames(
            csrc.node(), cdst.node(), missingfiles
        )
        copies.update(_filtercopies(dst_copies, base, csrc))

    # Look for additional amend-copies.
    amend_copies = copiesmod.getamendcopies(repo, cdst, base.p1())
    if amend_copies:
        repo.ui.debug("Loaded amend copytrace for %s" % cdst)
        for dst, src in _filtercopies(amend_copies, base, csrc).items():
            if dst not in copies:
                copies[dst] = src

    repo.ui.metrics.gauge("copytrace_copies", len(copies))
    return copies


def _filtercopies(copies, base, otherctx):
    """Remove uninteresting copies if a file is renamed in one side but not changed
    in the other side.

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
        otherctx.manifest()
        base.manifest()
        for fdst, fsrc in copies.items():
            if fsrc not in base:
                # Should not happen. Just be graceful in case something went
                # wrong.
                continue
            basenode = base[fsrc].filenode()
            if fsrc in otherctx and otherctx[fsrc].filenode() == basenode:
                continue
            newcopies[fdst] = fsrc
    return newcopies


def _dagcopytraceenabled(ui):
    return ui.configbool("copytrace", "dagcopytrace")

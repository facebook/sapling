# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# copies.py - copy detection for Mercurial
#
# Copyright 2008 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""fast copytracing

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

import codecs
from collections import defaultdict

from . import hgdemandimport, json, node, phases, pycompat


def _chain(src, dst, a, b):
    """chain two sets of copies a->b

    Assuming we have a commit graph like below::

        dst src
         | /
         |/
        base

    then:

    * `a` is a dict from `base` to `src`
    * `b` is a dict from `dst` to `base`

    This function returns a dict from `dst` to `src`.

    For example:
    * a is {"a": "x"}  # src rename a -> x
    * b is {"y": "a"}  # dst rename a -> y

    then the result will be {"y": "x"}
    """
    t = a.copy()
    for k, v in b.items():
        if v in t:
            # found a chain
            if t[v] != k:
                # file wasn't renamed back to itself
                t[k] = t[v]
            if v not in dst:
                # chain was a rename, not a copy
                del t[v]
        if v in src:
            # file is a copy of an existing file
            t[k] = v

    # remove criss-crossed copies
    for k, v in list(t.items()):
        if k in src and v in dst:
            del t[k]

    return t


def _dirstatecopies(d, match=None):
    ds = d._repo.dirstate
    c = ds.copies().copy()
    for k in list(c):
        if ds[k] not in "anm" or (match and not match(k)):
            del c[k]
    return c


def _reverse_copies(copies):
    """reverse the direction of the copies"""
    # For 1:n rename situations (e.g. hg cp a b; hg mv a c), we
    # arbitrarily pick one of the renames.
    return {v: k for k, v in copies.items()}


def pathcopies(x, y, match=None):
    """find {dst@y: src@x} copy mapping for directed compare"""
    if x == y or not x or not y:
        return {}

    dagcopytrace = y.repo()._dagcopytrace
    if y.rev() is None:
        dirstate_copies = _dirstatecopies(y, match)
        if x == y.p1():
            return dirstate_copies
        committed_copies = dagcopytrace.path_copies(x.node(), y.p1().node(), match)
        return _chain(x, y, committed_copies, dirstate_copies)

    if x.rev() is None:
        dirstate_copies = _reverse_copies(_dirstatecopies(x, match))
        if y == x.p1():
            return dirstate_copies
        committed_copies = dagcopytrace.path_copies(x.p1().node(), y.node(), match)
        return _chain(x, y, dirstate_copies, committed_copies)

    return dagcopytrace.path_copies(x.node(), y.node(), match)


def mergecopies(repo, cdst, csrc, base):
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
            return {}
        changedfiles.update(curr.files())
        curr = curr.p1()
        sourcecommitnum += 1
        if sourcecommitnum > sourcecommitlimit:
            return {}

    msrc = csrc.manifest()

    cp = pathcopies(base, csrc)
    for dst in list(cp.keys()):
        # dst and src paths will both be in csrc's "path space".
        # Convert dst path into mdst's "path space", fanning out.
        if grafted := msrc.graftedpaths(dst):
            src = cp.pop(dst)
            for path in grafted:
                cp[path] = src

    for dst, src in cp.items():
        if src in orig_cdst or dst in orig_cdst:
            copies[dst] = src

    missingfiles = []
    for src in changedfiles:
        # Fan out src file into the equivalent paths in dst.
        dst_files = msrc.graftedpaths(src) or [src]
        for dst in dst_files:
            # file is missing if it isn't present in the destination, but is present in
            # the base and present in the source.
            # Presence in the base is important to exclude added files, presence in the
            # source is important to exclude removed files.
            if dst not in mdst and src in base and src in csrc:
                missingfiles.append((dst, src))

    repo.ui.metrics.gauge("copytrace_missingfiles", len(missingfiles))
    if missingfiles and _dagcopytraceenabled(repo.ui):
        srconly, same = [], []
        for dst_path, src_path in missingfiles:
            if dst_path == src_path:
                same.append(dst_path)
            else:
                srconly.append(src_path)

        # Normal case - src and dest manifest use the same "path space", so just do a
        # single trace form csrc to cdst.
        copies |= repo._dagcopytrace.trace_renames(
            csrc.node(),
            cdst.node(),
            same,
        )

        # xdir missing files - use special xdir logic.
        copies |= xdir_copies(repo, csrc, cdst, srconly)

    # Look for additional amend-copies.
    amend_copies = getamendcopies(repo, cdst, base.p1())
    if amend_copies:
        repo.ui.debug("Loaded amend copytrace for %s" % cdst)
        for dst, src in amend_copies.items():
            if dst not in copies:
                copies[dst] = src

    repo.ui.metrics.gauge("copytrace_copies", len(copies))

    # For the xdir merge case, report copies in cdst's "path space".
    # This is more convenient for merge.py.
    for dst in copies.keys():
        src = copies[dst]
        copies[dst] = msrc.graftedpath(src, dst) or src

    return copies


def xdir_copies(repo, csrc, cdst, srcmissing):
    """Compute copies for cross-directory merging.

    Cross-directory differs from normal copy tracing because:

    1. Cross-directory merges don't necessarily have copy metadata for
       the files. If a user started with a plain "cp" instead of "sl cp",
       we want rename tracing for cross-directory merges to still work.

    2. The common ancestor of csrc and cdst is not necessarily far back enough
       to follow the branched directory history. For example:

    D # modify "foo/file"
    |
    C
    |
    B # rename "bar/file" to "bar/renamed"
    |
    A # branch "foo/" into "bar/"

    If we are on C and we graft D:foo into C:bar, the common ancestor
    for src=D^ and dst=C is C, but that is not far back enough to discover the
    rename from "bar/file" to "bar/renamed".
    """

    msrc = csrc.manifest()
    mdst = cdst.manifest()
    dag = repo.changelog.dag

    # Group missing paths by graft dest path (the "--to-path" option).
    dests = defaultdict(lambda: [])
    for srcpath in srcmissing:
        for dest in msrc.grafteddests(srcpath):
            dests[dest].append(srcpath)

    copies = {}
    for dstpath, srcpaths in dests.items():
        # Commit where "dstpath" was (most recently) created.
        cdstbase = repo.pathcreation(dstpath, dag.ancestors([cdst.node()]))

        srcdir = msrc.ungraftedpath(dstpath)
        # Commit where "srcdir" was (most recently) created.
        csrcbase = repo.pathcreation(srcdir, dag.ancestors([csrc.node()]))

        # Pull forward the bases so they don't extend too far into the history before the
        # branch point of the other dir.
        if dag.isancestor(csrcbase, cdstbase):
            csrcbase = dag.gcaone([cdstbase, csrc.node()])
        elif dag.isancestor(cdstbase, csrcbase):
            cdstbase = dag.gcaone([csrcbase, cdst.node()])

        # Trace renames on src side.
        src_copies = repo._dagcopytrace.trace_renames(
            csrc.node(),
            csrcbase,
            srcpaths,
        )

        # Fan out result paths into cdst "path space".
        dst_paths = []
        for path in src_copies.keys():
            for dst_path in msrc.graftedpaths(path):
                dst_paths.append(dst_path)

        # Continue rename trace on dst side.
        dst_copies = repo._dagcopytrace.trace_renames(
            cdstbase,
            cdst.node(),
            dst_paths,
        )

        # If a src in dst_copies is a dst in src_copies, stitch together.
        # This could happen if both sides have renamed.
        for dst in list(dst_copies.keys()):
            src = dst_copies[dst]
            src = msrc.ungraftedpath(src) or src
            if src in src_copies:
                dst_copies[dst] = src_copies[src]

        # Add rest of src_copies to copies if it exists in mdst.
        # We change the "dst" to be in the "path space" cdst instead of csrc.
        for dst, src in src_copies.items():
            for dst in msrc.graftedpaths(dst):
                if dst in mdst:
                    dst_copies[dst] = src

        copies |= dst_copies

    return copies


def _dagcopytraceenabled(ui):
    return ui.configbool("copytrace", "dagcopytrace")


def duplicatecopies(repo, wctx, rev, fromrev, skiprev=None):
    """reproduce copies from fromrev to rev in the dirstate

    If skiprev is specified, it's a revision that should be used to
    filter copy records. Any copies that occur between fromrev and
    skiprev will not be duplicated, even if they appear in the set of
    copies between fromrev and rev.
    """
    dagcopytrace = _get_dagcopytrace(repo, wctx, skiprev)
    for dst, src in pathcopies(repo[fromrev], repo[rev]).items():
        if (
            dagcopytrace
            and dst in repo[skiprev]
            and dagcopytrace.trace_rename(
                repo[skiprev].node(), repo[fromrev].node(), dst
            )
        ):
            continue
        wctx[dst].markcopied(src)


def _get_dagcopytrace(repo, wctx, skiprev):
    """this is for fixing empty commit issue in non-IMM case"""
    if (
        skiprev is None
        or wctx.isinmemory()
        or not repo.ui.configbool("copytrace", "skipduplicatecopies")
    ):
        return None
    return repo._dagcopytrace


def collect_amend_copies(ui, wctx, old, matcher):
    if not ui.configbool("copytrace", "enableamendcopytrace"):
        return {}
    return pathcopies(old, wctx, matcher)


def record_amend_copies(repo, amend_copies, old, amended_ctx):
    """Ccollect copytrace data on amend

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
    in mergecopies, this information will be merged with the rebase
    copytrace data to incorporate renames and copies made during the amend.
    """

    # Check if amend copytracing has been disabled.
    if not repo.ui.configbool("copytrace", "enableamendcopytrace"):
        return

    # Store the amend-copies against the amended context.
    if amend_copies:
        db, error = _opendbm(repo, "c")
        if db is None:
            # Database locked, can't record these amend-copies.
            repo.ui.log("copytrace", "Failed to open amendcopytrace db: %s" % error)
            return node

        # Merge in any existing amend copies from any previous amends.
        try:
            orig_data = db[old.node()]
        except KeyError:
            orig_data = "{}"
        except error as e:
            repo.ui.log(
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
            for (k, v) in orig_encoded.items()
        )

        # Copytrace information is not valid if it refers to a file that
        # doesn't exist in a commit.  We need to update or remove entries
        # that refer to files that might have only existed in the previous
        # amend commit.
        #
        # Find chained copies and renames (a -> b -> c) and collapse them to
        # (a -> c).  Delete the entry for b if this was a rename.
        for dst, src in amend_copies.items():
            if src in orig_amend_copies:
                amend_copies[dst] = orig_amend_copies[src]
                if src not in amended_ctx:
                    del orig_amend_copies[src]

        # Copy any left over copies from the previous context.
        for dst, src in orig_amend_copies.items():
            if dst not in amend_copies:
                amend_copies[dst] = src

        # Write out the entry for the new amend commit.
        encoded = dict(
            (
                pycompat.decodeutf8(codecs.encode(pycompat.encodeutf8(k), "base64")),
                pycompat.decodeutf8(codecs.encode(pycompat.encodeutf8(v), "base64")),
            )
            for (k, v) in amend_copies.items()
        )
        db[amended_ctx.node()] = json.dumps(encoded)
        try:
            db.close()
        except Exception as e:
            # Database corruption.  Not much we can do, so just log.
            repo.ui.log("copytrace", "Failed to close amendcopytrace db: %s" % e)

    return node


# Note: dbm._Database does not exist.
def _opendbm(repo, flag):
    """Open the dbm of choice.

    On some platforms, dbm is available, on others it's not,
    but gdbm is unfortunately not available everywhere, like on Windows.
    """
    with hgdemandimport.deactivated():
        import dbm

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


def getamendcopies(repo, dest, ancestor):
    if not repo.ui.configbool("copytrace", "enableamendcopytrace"):
        return {}

    db, error = _opendbm(repo, "r")
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
            for (k, v) in encoded.items()
        )
    except Exception:
        repo.ui.log("copytrace", "Failed to load amend copytrace for %s" % dest.hex())
        return {}
    finally:
        try:
            db.close()
        except error:
            pass

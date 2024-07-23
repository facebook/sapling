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

import codecs

from . import git, hgdemandimport, json, node, pathutil, phases, pycompat, util


def _findlimit(repo, a, b):
    """
    Find the earliest revision that's an ancestor of a or b but not both, except
    in the case where a or b is an ancestor of the other.
    """
    if a is None:
        a = repo.revs("p1()").first()
    if b is None:
        b = repo.revs("p1()").first()
    if a is None or b is None or not repo.revs("ancestor(%d, %d)", a, b):
        return None

    return repo.revs("only(%d, %d) + only(%d, %d) + %d + %d", a, b, b, a, a, b).min()


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
    for k, v in pycompat.iteritems(b):
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


def mergecopies(repo, c1, c2, base):
    # This function is wrapped by copytrace.mergecopies,
    return {}


def duplicatecopies(repo, wctx, rev, fromrev, skiprev=None):
    """reproduce copies from fromrev to rev in the dirstate

    If skiprev is specified, it's a revision that should be used to
    filter copy records. Any copies that occur between fromrev and
    skiprev will not be duplicated, even if they appear in the set of
    copies between fromrev and rev.
    """
    dagcopytrace = _get_dagcopytrace(repo, wctx, skiprev)
    for dst, src in pycompat.iteritems(pathcopies(repo[fromrev], repo[rev])):
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

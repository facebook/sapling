# phabstatus.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import os

from edenscm.mercurial import (
    cmdutil,
    extensions,
    obsutil,
    pycompat,
    registrar,
    util as hgutil,
)
from edenscm.mercurial.i18n import _

from .extlib.phabricator import arcconfig, diffprops, graphql


COMMITTEDSTATUS = "Committed"


def memoize(f):
    """
    NOTE: This is a hack
    if f args are like (a, b1, b2, b3) and returns [o1, o2, o3] where
    o1, o2, o3 are output of f respectively for (a, b1), (a, b2) and
    (a, b3) then we memoize f(a, b1, b2, b3)'s result but also
    f(a, b1) => o1 , f(a, b2) => o2 and f(a, b3) => o3.
    Example:

    >>> partialsum = lambda a, *b: [a + bn for bn in b]
    >>> partialsum = memoize(partialsum)

    Create a class that wraps the integer '3', otherwise we cannot add
    _phabstatuscache to it for the test
    >>> class IntWrapperClass(int):
    ...     def __new__(cls, *args, **kwargs):
    ...         return  super(IntWrapperClass, cls).__new__(cls, 3)

    >>> three = IntWrapperClass()
    >>> partialsum(three, 1, 2, 3)
    [4, 5, 6]

    As expected, we have 4 entries in the cache for a call like f(a, b, c, d)
    >>> pp(three._phabstatuscache)
    {(3, 1): [4], (3, 1, 2, 3): [4, 5, 6], (3, 2): [5], (3, 3): [6]}
    """

    def helper(*args):
        repo = args[0]
        if not hgutil.safehasattr(repo, "_phabstatuscache"):
            repo._phabstatuscache = {}
        if args not in repo._phabstatuscache:
            u = f(*args)
            repo._phabstatuscache[args] = u
            if isinstance(u, list):
                revs = args[1:]
                for x, r in enumerate(revs):
                    repo._phabstatuscache[(repo, r)] = [u[x]]
        return repo._phabstatuscache[args]

    return helper


def _fail(repo, diffids, *msgs):
    for msg in msgs:
        repo.ui.warn(msg)
    return ["Error"] * len(diffids)


@memoize
def getdiffstatus(repo, *diffid):
    """Perform a Conduit API call to get the diff status

    Returns status of the diff"""

    if not diffid:
        return []
    timeout = repo.ui.configint("ssl", "timeout", 10)
    signalstatus = repo.ui.configbool("ssl", "signal_status", True)
    ca_certs = repo.ui.configpath("web", "cacerts")

    try:
        client = graphql.Client(
            repodir=pycompat.getcwd(), ca_bundle=ca_certs, repo=repo
        )
        statuses = client.getrevisioninfo(timeout, signalstatus, diffid)
    except arcconfig.ArcConfigError as ex:
        msg = _(
            "arcconfig configuration problem. No diff information can be " "provided.\n"
        )
        hint = _("Error info: %s\n") % str(ex)
        ret = _fail(repo, diffid, msg, hint)
        return ret
    except graphql.ClientError as ex:
        msg = _(
            "Error talking to phabricator. No diff information can be " "provided.\n"
        )
        hint = _("Error info: %s\n") % str(ex)
        ret = _fail(repo, diffid, msg, hint)
        return ret

    # This makes the code more robust in case we don't learn about any
    # particular revision
    result = []
    for diff in diffid:
        matchingresponse = statuses.get(str(diff))
        if not matchingresponse:
            result.append("Error")
        else:
            result.append(matchingresponse)
    return result


def populateresponseforphab(repo, diffnum):
    """:populateresponse: Runs the memoization function
        for use of phabstatus and sync status
    """
    if not hgutil.safehasattr(repo, "_phabstatusrevs"):
        return

    if (
        hgutil.safehasattr(repo, "_phabstatuscache")
        and (repo, diffnum) in repo._phabstatuscache
    ):
        # We already have cached data for this diff
        return

    next_revs = repo._phabstatusrevs.peekahead()
    if repo._phabstatusrevs.done:
        # repo._phabstatusrevs doesn't have anything else to process.
        # Remove it so we will bail out earlier next time.
        del repo._phabstatusrevs

    alldiffnumbers = [getdiffnum(repo, repo.unfiltered()[rev]) for rev in next_revs]
    okdiffnumbers = set(d for d in alldiffnumbers if d is not None)
    # Make sure we always include the requested diff number
    okdiffnumbers.add(diffnum)
    # To populate the cache, the result will be used by the templater
    getdiffstatus(repo, *okdiffnumbers)


templatekeyword = registrar.templatekeyword()


@templatekeyword("phabstatus")
def showphabstatus(repo, ctx, templ, **args):
    """String. Return the diff approval status for a given hg rev
    """
    diffnum = getdiffnum(repo, ctx)
    if diffnum is None:
        return None
    populateresponseforphab(repo, diffnum)

    result = getdiffstatus(repo, diffnum)[0]
    if isinstance(result, dict) and "status" in result:
        if result.get("is_landing"):
            return "Landing"
        else:
            return result.get("status")
    else:
        return "Error"


@templatekeyword("phabsignalstatus")
def showphabsignalstatus(repo, ctx, templ, **args):
    """String. Return the diff Signal status for a given hg rev
    """
    diffnum = getdiffnum(repo, ctx)
    if diffnum is None:
        return None
    populateresponseforphab(repo, diffnum)

    result = getdiffstatus(repo, diffnum)[0]
    if isinstance(result, dict):
        return result.get("signal_status")


"""
in order to determine whether the local changeset is in sync with the
remote one we compare the hash of the current changeset with the one we
get from the remote (phabricator) repo. There are three different cases
and we deal with them seperately.
1) If this is the first revision in a diff: We look at the count field and
understand that this is the first changeset, so we compare the hash we get
from remote repo with the predessesor's hash from the local changeset. The
reason for that is the D number is ammended on the changeset after it is
sent to phabricator.
2) If this is the last revision, i.e. it is alread committed: Then we
don't say anything. All good.
3) If this is a middle revision: Then we compare the hashes as regular.
"""


@templatekeyword("syncstatus")
def showsyncstatus(repo, ctx, templ, **args):
    """String. Return whether the local revision is in sync
        with the remote (phabricator) revision
    """
    diffnum = getdiffnum(repo, ctx)
    if diffnum is None:
        return None

    populateresponseforphab(repo, diffnum)
    results = getdiffstatus(repo, diffnum)
    try:
        result = results[0]
        remote = result["hash"]
        status = result["status"]
        count = int(result["count"])
    except (IndexError, KeyError, ValueError, TypeError):
        # We got no result back, or it did not contain all required fields
        return "Error"

    local = ctx.hex()
    if local == remote:
        return "sync"
    elif count == 1:
        precursors = list(obsutil.allpredecessors(repo.obsstore, [ctx.node()]))
        hashes = [
            repo.unfiltered()[h].hex() for h in precursors if h in repo.unfiltered()
        ]
        # hashes[0] is the current
        # hashes[1] is the previous
        if len(hashes) > 1 and hashes[1] == remote:
            return "sync"
        else:
            return "unsync"
    elif status == "Committed":
        return "committed"
    else:
        return "unsync"


def getdiffnum(repo, ctx):
    return diffprops.parserevfromcommitmsg(ctx.description())


class PeekaheadRevsetIter(object):
    """
    PeekaheadRevsetIter is a helper class that wraps a revision set iterator,
    and allows the phabstatus code to peek ahead in the list as the logging
    code is iterating through it.

    The main logging code uses the normal iterator interface (next()) to
    iterate through this revision set.

    The phabstatus code will call peekahead() to peek ahead in the list, so it
    can query information for multiple revisions at once, rather than only
    processing them one at a time as the logging code requests them.
    """

    def __init__(self, revs, chunksize=30):
        self.mainiter = iter(revs)
        # done is set to true once mainiter has thrown StopIteration
        self.done = False

        # chunk is the peekahead chunk we have returned from peekahead().
        self.chunk = list()
        # chunk_idx represents how far into self.chunk() the main iteration
        # code has seen via the next() API.
        self.chunk_idx = 0
        self.chunksize = chunksize

    def next(self):
        if self.chunk_idx < len(self.chunk):
            # We still have data remaining in the peekahead chunk to return
            result = self.chunk[self.chunk_idx]
            self.chunk_idx += 1
            if self.chunk_idx >= len(self.chunk):
                self.chunk = list()
                self.chunk_idx = 0
            return result

        if self.done:
            raise StopIteration()

        try:
            return next(self.mainiter)
        except StopIteration:
            self.done = True
            raise

    def peekahead(self, chunksize=None):
        chunksize = chunksize or self.chunksize
        while len(self.chunk) < chunksize and not self.done:
            try:
                self.chunk.append(next(self.mainiter))
            except StopIteration:
                self.done = True

        return self.chunk


def _getlogrevs(orig, repo, pats, opts):
    # Call the original function
    revs, expr, filematcher = orig(repo, pats, opts)

    # Wrap the revs result so that iter(revs) returns a PeekaheadRevsetIter()
    # the first time it is invoked, and sets repo._phabstatusrevs so that the
    # phabstatus code will be able to peek ahead at the revs to be logged.
    orig_type = revs.__class__

    class wrapped_class(type(revs)):
        def __iter__(self):
            # The first time __iter__() is called, return a
            # PeekaheadRevsetIter(), and assign it to repo._phabstatusrevs
            revs.__class__ = orig_type
            # By default, peek ahead 30 revisions at a time
            peekahead = repo.ui.configint("phabstatus", "logpeekahead", 30)
            repo._phabstatusrevs = PeekaheadRevsetIter(revs, peekahead)
            return repo._phabstatusrevs

        _is_phabstatus_wrapped = True

    if not hgutil.safehasattr(revs, "_is_phabstatus_wrapped"):
        revs.__class__ = wrapped_class

    return revs, expr, filematcher


class PeekaheadList(object):
    """
    PeekaheadList exposes peekahead() and done just like PeekaheadRevsetIter,
    but wraps a simple list instead of a revset generator.  peekahead() returns
    the full list.
    """

    def __init__(self, revs):
        self.revs = revs
        self.done = False

    def peekahead(self):
        self.done = True
        return self.revs


def _getsmartlogdag(orig, ui, repo, revs, *args):
    # smartlog just uses a plain list for its revisions, and not an
    # abstractsmartset type.  We just save a copy of it.
    repo._phabstatusrevs = PeekaheadList(revs)
    return orig(ui, repo, revs, *args)


def extsetup(ui):
    # Wrap the APIs used to get the revisions for "hg log" so we
    # can peekahead into the rev list and query phabricator for multiple diffs
    # at once.
    extensions.wrapfunction(cmdutil, "getlogrevs", _getlogrevs)
    extensions.wrapfunction(cmdutil, "getgraphlogrevs", _getlogrevs)

    # Also wrap the APIs used by smartlog
    def _smartlogloaded(loaded):
        smartlog = None
        try:
            smartlog = extensions.find("smartlog")
        except KeyError:
            pass
        if smartlog:
            extensions.wrapfunction(smartlog, "getdag", _getsmartlogdag)

    extensions.afterloaded("smartlog", _smartlogloaded)

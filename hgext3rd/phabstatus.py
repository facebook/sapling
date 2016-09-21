# phabstatus.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import templatekw, extensions
from mercurial import util as hgutil
from mercurial.i18n import _
from mercurial import obsolete

import re
import subprocess
import os
import json

from phabricator import (
    arcconfig,
    conduit,
    diffprops,
)

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
        if not hgutil.safehasattr(repo, '_phabstatuscache'):
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
    timeout = repo.ui.configint('ssl', 'timeout', 5)

    try:
        resp = conduit.call_conduit('differential.querydiffhashes',
                {'revisionIDs': diffid},
                timeout=timeout)

    except conduit.ClientError as ex:
        msg = _('Error talking to phabricator. No diff information can be '
                'provided.\n')
        hint = _("Error info: ") + str(ex)
        return _fail(repo, diffid, msg, hint)
    except arcconfig.ArcConfigError as ex:
        msg = _('arcconfig configuration problem. No diff information can be '
                'provided.\n')
        hint = _("Error info: ") + str(ex)
        return _fail(repo, diffid, msg, hint)

    if not resp:
        resp = {}

    # This makes the code more robust in case conduit does not return
    # what we need
    result = []
    for diff in diffid:
        matchingresponse = resp.get(diff)
        if not matchingresponse:
            result.append("Error")
        else:
            result.append(matchingresponse)
    return result

def populateresponseforphab(repo, ctx):
    """:populateresponse: Runs the memoization function
        for use of phabstatus and sync status
    """
    if hgutil.safehasattr(repo, '_smartlogrevs'):
        alldiffnumbers = [getdiffnum(repo, repo[rev])
                          for rev in repo._smartlogrevs]
        okdiffnumbers = [d for d in alldiffnumbers if d is not None]
        # To populate the cache, the result will be used by the templater
        getdiffstatus(repo, *okdiffnumbers)
        # Do this once per smartlog call, not for every revs to be displayed
        del repo._smartlogrevs

def showphabstatus(repo, ctx, templ, **args):
    """:phabstatus: String. Return the diff approval status for a given hg rev
    """
    populateresponseforphab(repo, ctx)

    diffnum = getdiffnum(repo, ctx)
    if diffnum is not None:
        result = getdiffstatus(repo, diffnum)[0]
        if isinstance(result, dict) and "status" in result:
            return result.get("status")
        else:
            return "Error"

"""
In order to determine whether the local changeset is in sync with the
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
def showsyncstatus(repo, ctx, templ, **args):
    """:syncstatus: String. Return whether the local revision is in sync
        with the remote (phabricator) revision
    """
    populateresponseforphab(repo, ctx)

    diffnum = getdiffnum(repo, ctx)
    local = ctx.hex()
    if diffnum is not None:
        result = getdiffstatus(repo, diffnum)[0]

        if isinstance(result, dict) and "hash" in result \
        and "status" in result and "count" in result:
            remote = getdiffstatus(repo, diffnum)[0].get("hash")
            status = getdiffstatus(repo, diffnum)[0].get("status")
            count = int(getdiffstatus(repo, diffnum)[0].get("count"))

            if local == remote:
                return "sync"
            elif count == 1:
                precursors = list(obsolete.allprecursors(repo.obsstore,
                    [ctx.node()]))
                hashes = [repo.unfiltered()[h].hex() for h in precursors]
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
        else:
            return "Error"

def getdiffnum(repo, ctx):
    return diffprops.parserevfromcommitmsg(ctx.description())

def _getdag(orig, *args):
    repo = args[1]
    # We retain the smartlogrevision, this way showphabstatus knows that there
    # are multiple revisions to resolve
    repo._smartlogrevs = args[2]
    return orig(*args)

def extsetup(ui):
    templatekw.keywords['phabstatus'] = showphabstatus
    templatekw.keywords['syncstatus'] = showsyncstatus
    smartlog = extensions.find("smartlog")
    extensions.wrapfunction(smartlog, 'getdag', _getdag)

# phabstatus.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import templatekw, extensions
from mercurial import util as hgutil
from mercurial.i18n import _

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
        resp = conduit.call_conduit('differential.query', {'ids': diffid},
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
        resp = []

    # This makes the code more robust in case conduit does not return
    # what we need
    result = []
    for diff in diffid:
        matchingresponse = [r for r in resp
                              if r.get("id", None) == int(diff)]
        if not matchingresponse:
            result.append("Error")
        else:
            result.append(matchingresponse[0].get('statusName'))
    return result

def showphabstatus(repo, ctx, templ, **args):
    """:phabstatus: String. Return the diff approval status for a given hg rev
    """
    if hgutil.safehasattr(repo, '_smartlogrevs'):
        alldiffnumbers = [getdiffnum(repo, repo[rev])
                          for rev in repo._smartlogrevs]
        okdiffnumbers = [d for d in alldiffnumbers if d is not None]
        # To populate the cache, the result will be used by the templater
        getdiffstatus(repo, *okdiffnumbers)
        # Do this once per smartlog call, not for every revs to be displayed
        del repo._smartlogrevs

    diffnum = getdiffnum(repo, ctx)
    if diffnum is not None:
        return getdiffstatus(repo, diffnum)[0]

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
    smartlog = extensions.find("smartlog")
    extensions.wrapfunction(smartlog, 'getdag', _getdag)

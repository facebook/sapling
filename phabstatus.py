# phabstatus.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import templatekw, extensions
from mercurial import util as hgutil

import re
from pprint import pprint as pp
import subprocess
import os
import json
import logging

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

@memoize
def getdiffstatus(repo, *diffid):
    """Perform a Conduit API call by shelling out to `arc`

    Returns status of the diff"""

    try:
        proc = subprocess.Popen(['arc', 'call-conduit', 'differential.query'],
                     stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                     preexec_fn=os.setsid)
        input = json.dumps({'ids': diffid})
        repo.ui.debug("[diffrev] echo '%s' | "
                      "arc call-conduit differential.query\n" %
                      input)
        proc.stdin.write(input)
        proc.stdin.close()
        resp = proc.stdout.read()
        jsresp = json.loads(resp)
        if not jsresp:
            return 'Could not decode Conduit response'

        resp = jsresp.get('response')
        if not resp:
            error = jsresp.get('errorMessage', 'unknown error')
            return error

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
    except Exception as e:
        return 'Could not not call "arc call-conduit": %s' % e

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
    descr = ctx.description()
    match = re.search('Differential Revision: https://phabricator.fb.com/(D\d+)'
                      , descr)
    revstr = match.group(1) if match else ''
    if revstr.startswith('D') and revstr[1:].isdigit():
        return revstr[1:]
    return None

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

# phabstatus.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import templatekw
from mercurial import util as hgutil

import re
import subprocess
import os
import json
import logging

def memoize(f):
    def helper(*args):
        repo = args[0]
        if not hgutil.safehasattr(repo, '_phabstatuscache'):
            repo._phabstatuscache = {}
        if args not in repo._phabstatuscache:
            repo._phabstatuscache[args] = f(*args)
        return repo._phabstatuscache[args]
    return helper

@memoize
def getdiffstatus(repo, diffid):
    """Perform a Conduit API call by shelling out to `arc`

    Returns status of the diff"""

    try:
        proc = subprocess.Popen(['arc', 'call-conduit', 'differential.query'],
                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, preexec_fn=os.setsid)
        input = json.dumps({'ids': [ diffid ]})
        repo.ui.debug("[diffrev] echo '%s' | arc call-conduit differential.query\n" %
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
        return resp[0].get('statusName')
    except Exception, e:
        return 'Could not not call "arc call-conduit": %s' % e

def showphabstatus(repo, ctx, templ, **args):
    """:phabstatus: String. Return the diff approval status for a given hg rev
    """
    descr = ctx.description()
    match = re.search('Differential Revision: https://phabricator.fb.com/(D\d+)', descr)
    revstr = match.group(1) if match else ''
    if revstr.startswith('D') and revstr[1:].isdigit():
        return getdiffstatus(repo, revstr[1:])


def extsetup(ui):
    templatekw.keywords['phabstatus'] = showphabstatus


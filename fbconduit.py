# fbconduit.py
#
# An extension to query remote servers for extra information via conduit RPC
#
# Copyright 2015 Facebook, Inc.

from mercurial import templater, extensions, revset, templatekw, node
from mercurial.i18n import _

import re
import json
from urllib import urlencode
import httplib

conduit_host = None
conduit_path = None
connection = None

MAX_CONNECT_RETRIES = 3

class ConduitError(Exception):
    pass

class HttpError(Exception):
    pass

githashre = re.compile('g([0-9a-fA-F]{40,40})')
fbsvnhash = re.compile('r[A-Z]+(\d+)')

def extsetup(ui):
    global conduit_host, conduit_path, conduit_protocol
    conduit_host = ui.config('fbconduit', 'host')
    conduit_path = ui.config('fbconduit', 'path')
    conduit_protocol = ui.config('fbconduit', 'protocol')

    if not conduit_host:
        ui.warn('No conduit host specified in config; disabling fbconduit\n')
        return
    if not conduit_protocol:
        conduit_protocol = 'https'
    templater.funcs['mirrornode'] = mirrornode
    templatekw.keywords['gitnode'] = showgitnode

    revset.symbols['gitnode'] = gitnode
    extensions.wrapfunction(revset, 'stringset', overridestringset)
    revset.symbols['stringset'] = revset.stringset
    revset.methods['string'] = revset.stringset
    revset.methods['symbol'] = revset.stringset

def _call_conduit(method, **kwargs):
    global connection, conduit_host, conduit_path, conduit_protocol

    # start connection
    if connection is None:
        if conduit_protocol == 'https':
            connection = httplib.HTTPSConnection(conduit_host)
        elif conduit_protocol == 'http':
            connection = httplib.HTTPConnection(conduit_host)

    # send request
    path = conduit_path + method
    args = urlencode({'params': json.dumps(kwargs)})
    for attempt in xrange(MAX_CONNECT_RETRIES):
        try:
            connection.request('POST', path, args, {'Connection': 'Keep-Alive'})
            break;
        except httplib.HTTPException as e:
            connection.connect()
    else:
        raise e

    # read http response
    response = connection.getresponse()
    if response.status != 200:
        raise HttpError(response.reason)
    result = response.read()

    # strip jsonp header and parse
    assert result.startswith('for(;;);')
    result = json.loads(result[8:])

    # check for conduit errors
    if result['error_code']:
        raise ConduitError(result['error_info'])

    # return RPC result
    return result['result']

    # don't close the connection b/c we want to avoid the connection overhead

def mirrornode(ctx, mapping, args):
    '''template: find this commit in other repositories'''

    reponame = mapping['repo'].ui.config('fbconduit', 'reponame')
    if not reponame:
        # We don't know who we are, so we can't ask for a translation
        return ''

    if mapping['ctx'].mutable():
        # Local commits don't have translations
        return ''

    node = mapping['ctx'].hex()
    args = [f(ctx, mapping, a) for f, a in args]
    if len(args) == 1:
        torepo, totype = reponame, args[0]
    else:
        torepo, totype = args

    try:
        result = _call_conduit('scmquery.get.mirrored.revs',
            from_repo=reponame,
            from_scm='hg',
            to_repo=torepo,
            to_scm=totype,
            revs=[node]
        )
    except ConduitError as e:
        if 'unknown revision' not in str(e.args):
            mapping['repo'].ui.warn(str(e.args) + '\n')
        return ''
    return result.get(node, '')

def showgitnode(repo, ctx, templ, **args):
    """Return the git revision corresponding to a given hg rev"""
    reponame = repo.ui.config('fbconduit', 'reponame')
    if not reponame:
        # We don't know who we are, so we can't ask for a translation
        return ''
    backingrepos = repo.ui.configlist('fbconduit', 'backingrepos', default=[reponame])

    if ctx.mutable():
        # Local commits don't have translations
        return ''

    matches = []
    for backingrepo in backingrepos:
        try:
            result = _call_conduit('scmquery.get.mirrored.revs',
                from_repo=reponame,
                from_scm='hg',
                to_repo=backingrepo,
                to_scm='git',
                revs=[ctx.hex()]
            )
            githash = result[ctx.hex()]
            if githash != "":
                matches.append((backingrepo, githash))
        except ConduitError:
            pass

    if len(matches) == 0:
        return ''
    elif len(backingrepos) == 1:
        return matches[0][1]
    else:
        # in case it's not clear, the sort() is to ensure the output is in a
        # deterministic order.
        matches.sort()
        return "; ".join(["{0}: {1}".format(*match)
                          for match in matches])

def gitnode(repo, subset, x):
    """``gitnode(id)``
    Return the hg revision corresponding to a given git rev."""
    l = revset.getargs(x, 1, 1, _("id requires one argument"))
    n = revset.getstring(l[0], _("id requires a string"))

    reponame = repo.ui.config('fbconduit', 'reponame')
    if not reponame:
        # We don't know who we are, so we can't ask for a translation
        return subset.filter(lambda r: false)
    backingrepos = repo.ui.configlist('fbconduit', 'backingrepos', default=[reponame])

    peerpath = repo.ui.expandpath('default')
    translationerror = False
    for backingrepo in backingrepos:
        try:
            result = _call_conduit('scmquery.get.mirrored.revs',
                from_repo=backingrepo,
                from_scm='git',
                to_repo=reponame,
                to_scm='hg',
                revs=[n]
            )
            hghash = result[n]
            if hghash != '':
                break
        except ConduitError as e:
            pass
    else:
        translationerror = True

    if translationerror or result[n] == "":
        repo.ui.warn("Could not translate revision {0}.\n".format(n))
        return subset.filter(lambda r: False)

    rn = repo[node.bin(result[n])].rev()
    return subset.filter(lambda r: r == rn)

def overridestringset(orig, repo, subset, x):
    svnrev = fbsvnhash.match(x)
    if svnrev and not x in repo:
        try:
            extensions.find('hgsubversion')
            meta = repo.svnmeta()

            desiredrevision = int(svnrev.group(1))
            # For some odd reason, the key is a tuple instead of a revision num
            # The second member always seems to be None
            revmapkey = (desiredrevision, None)
            hghash = meta.revmap.get(revmapkey)
            if hghash:
                return orig(repo, subset, hghash)

        except KeyError:
            pass

    m = githashre.match(x)
    if m is not None:
        return gitnode(repo, subset, ('string', m.group(1)))
    return orig(repo, subset, x)

# fbconduit.py
#
# An extension to query remote servers for extra information via conduit RPC
#
# Copyright 2015 Facebook, Inc.

from mercurial import templater
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

def extsetup(ui):
    global conduit_host, conduit_path
    conduit_host = ui.config('fbconduit', 'host')
    conduit_path = ui.config('fbconduit', 'path')
    
    if not conduit_host:
        ui.warn('No conduit host specified in config; disabling fbconduit\n')
        return
    templater.funcs['mirrornode'] = mirrornode

def _call_conduit(method, **kwargs):
    global connection, conduit_host, conduit_path

    # start connection
    if connection is None:
        connection = httplib.HTTPSConnection(conduit_host)

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

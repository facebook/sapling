# phabricator.py - simple Phabricator integration
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""simple Phabricator integration

Config::

    [phabricator]
    # Phabricator URL
    url = https://phab.example.com/

    # API token. Get it from https://$HOST/conduit/login/
    token = cli-xxxxxxxxxxxxxxxxxxxxxxxxxxxx
"""

from __future__ import absolute_import

import json

from mercurial.i18n import _
from mercurial import (
    error,
    registrar,
    url as urlmod,
    util,
)

cmdtable = {}
command = registrar.command(cmdtable)

def urlencodenested(params):
    """like urlencode, but works with nested parameters.

    For example, if params is {'a': ['b', 'c'], 'd': {'e': 'f'}}, it will be
    flattened to {'a[0]': 'b', 'a[1]': 'c', 'd[e]': 'f'} and then passed to
    urlencode. Note: the encoding is consistent with PHP's http_build_query.
    """
    flatparams = util.sortdict()
    def process(prefix, obj):
        items = {list: enumerate, dict: lambda x: x.items()}.get(type(obj))
        if items is None:
            flatparams[prefix] = obj
        else:
            for k, v in items(obj):
                if prefix:
                    process('%s[%s]' % (prefix, k), v)
                else:
                    process(k, v)
    process('', params)
    return util.urlreq.urlencode(flatparams)

def readurltoken(repo):
    """return conduit url, token and make sure they exist

    Currently read from [phabricator] config section. In the future, it might
    make sense to read from .arcconfig and .arcrc as well.
    """
    values = []
    section = 'phabricator'
    for name in ['url', 'token']:
        value = repo.ui.config(section, name)
        if not value:
            raise error.Abort(_('config %s.%s is required') % (section, name))
        values.append(value)
    return values

def callconduit(repo, name, params):
    """call Conduit API, params is a dict. return json.loads result, or None"""
    host, token = readurltoken(repo)
    url, authinfo = util.url('/'.join([host, 'api', name])).authinfo()
    urlopener = urlmod.opener(repo.ui, authinfo)
    repo.ui.debug('Conduit Call: %s %s\n' % (url, params))
    params = params.copy()
    params['api.token'] = token
    request = util.urlreq.request(url, data=urlencodenested(params))
    body = urlopener.open(request).read()
    repo.ui.debug('Conduit Response: %s\n' % body)
    parsed = json.loads(body)
    if parsed.get(r'error_code'):
        msg = (_('Conduit Error (%s): %s')
               % (parsed[r'error_code'], parsed[r'error_info']))
        raise error.Abort(msg)
    return parsed[r'result']

@command('debugcallconduit', [], _('METHOD'))
def debugcallconduit(ui, repo, name):
    """call Conduit API

    Call parameters are read from stdin as a JSON blob. Result will be written
    to stdout as a JSON blob.
    """
    params = json.loads(ui.fin.read())
    result = callconduit(repo, name, params)
    s = json.dumps(result, sort_keys=True, indent=2, separators=(',', ': '))
    ui.write('%s\n' % s)

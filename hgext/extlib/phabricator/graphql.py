# graphql.py
#
# A library function to call a phabricator graphql RPC.
# This replaces the Conduit methods

from __future__ import absolute_import

import json
import operator

from mercurial import (
    encoding,
    pycompat,
    util
)

from . import (
    arcconfig,
    phabricator_graphql_client,
    phabricator_graphql_client_urllib
)

urlreq = util.urlreq

class ClientError(Exception):
    def __init__(self, code, msg):
        Exception.__init__(self, msg)
        self.code = code

class Client(object):
    def __init__(self, repodir=None, ca_bundle=None, repo=None):
        if not repodir:
            repodir=pycompat.getcwd()
        self._mock = 'HG_ARC_CONDUIT_MOCK' in encoding.environ
        if self._mock:
            with open(encoding.environ['HG_ARC_CONDUIT_MOCK'], 'r') as f:
                self._mocked_responses = json.load(f)
                # reverse since we want to use pop but still get items in
                # original order
                self._mocked_responses.reverse()

        self._host = None
        self._user = None
        self._cert = None
        self._oauth = None
        self.ca_bundle = ca_bundle or True
        self._applyarcconfig(arcconfig.loadforpath(repodir))
        if not self._mock:
            app_id = repo.ui.config('phabricator', 'graphql_app_id')
            app_token = repo.ui.config('phabricator', 'graphql_app_token')
            self._host = repo.ui.config('phabricator', 'graphql_host')
            self._client = phabricator_graphql_client.PhabricatorGraphQLClient(
                phabricator_graphql_client_urllib.
                PhabricatorGraphQLClientRequests(), self._cert, self._oauth,
                self._user, 'phabricator', self._host, app_id, app_token)

    def _applyarcconfig(self, config):
        self._host = config.get('graphql_uri', self._host)
        if 'OVERRIDE_GRAPHQL_URI' in encoding.environ:
            self._host = encoding.environ['OVERRIDE_GRAPHQL_URI']
        try:
            hostconfig = config['hosts'][self._host]
            self._user = hostconfig['user']
            self._cert = hostconfig.get('cert', None)
            self._oauth = hostconfig.get('oauth', None)
        except KeyError:
            try:
                hostconfig = config['hosts'][config['hosts'].keys()[0]]
                self._user = hostconfig['user']
                self._cert = hostconfig.get('cert', None)
                self._oauth = hostconfig.get('oauth', None)
            except KeyError:
                pass

        if self._cert is None and self._oauth is None:
            raise arcconfig.ArcConfigError(
                'arcrc is missing user '
                'credentials for host %s.  use '
                '"arc install-certificate" to fix.' % self._host)

    def _normalizerevisionnumbers(self, *revision_numbers):
        rev_numbers = []
        if isinstance(revision_numbers, str):
            return [int(revision_numbers)]
        for r in revision_numbers:
            if isinstance(r, list) or isinstance(r, tuple):
                for rr in r:
                    rev_numbers.extend(rr)
            else:
                rev_numbers.append(int(r))
        return [int(x) for x in rev_numbers]

    def getrevisioninfo(self, timeout, *revision_numbers):
        rev_numbers = self._normalizerevisionnumbers(revision_numbers)
        if self._mock:
            ret = self._mocked_responses.pop()
        else:
            params = { 'params': { 'numbers': rev_numbers } }
            ret = self._client.query(timeout, self._getquery(), params)
        return self._processrevisioninfo(ret)

    def _getquery(self):
        return '''
        query RevisionQuery(
          $params: [DifferentialRevisionQueryParams!]!
        ) {
          query: differential_revision_query(query_params: $params) {
            results {
              nodes {
                number
                diff_status_name
                latest_active_diff {
                  local_commit_info: diff_properties (
                    property_names: ["local:commits"]
                  ) {
                    nodes {
                      property_value
                    }
                  }
                }
                created_time
                updated_time
                is_landing
                differential_diffs {
                  count
                }
              }
            }
          }
        }
        '''

    def _processrevisioninfo(self, ret):
        try:
            errormsg = ret['errors'][0]['message']
            raise ClientError(None, errormsg)
        except (KeyError, TypeError):
            pass

        infos = {}
        try:
            nodes = ret['data']['query'][0]['results']['nodes']
            for node in nodes:
                info = {}
                infos[str(node['number'])] = info

                status = node['diff_status_name']
                # GraphQL uses "Closed" but Conduit used "Committed" so let's
                # not change the naming
                if status == 'Closed':
                    status = 'Committed'
                info['status'] = status
                info['created'] = node['created_time']
                info['updated'] = node['updated_time']
                info['is_landing'] = node['is_landing']

                if 'latest_active_diff' not in node:
                    continue
                active_diff = node['latest_active_diff']

                info['count'] = node['differential_diffs']['count']

                localcommitnode = active_diff['local_commit_info']['nodes']
                if localcommitnode is not None and len(localcommitnode) == 1:
                    localcommits = json.loads(localcommitnode[0][
                                                            'property_value'])
                    localcommits = sorted(localcommits.values(),
                                          key=operator.itemgetter('time'),
                                          reverse=True)
                    info['hash'] = localcommits[0].get('commit', None)

        except (TypeError, KeyError):
            raise ClientError(None, 'Unexpected graphql response format')

        return infos

# conduit.py
#
# A library function to call a phabricator conduit RPC.
# It's different from fbconduit in that this is an authenticated
# conduit client.

from __future__ import absolute_import

import contextlib
import hashlib
import json
import os
import time
import warnings

import urllib3

from mercurial import util

from . import arcconfig

urlreq = util.urlreq

DEFAULT_URL = 'https://phabricator.intern.facebook.com/api/'
DEFAULT_TIMEOUT = 60
mocked_responses = None

class ClientError(Exception):
    def __init__(self, code, msg):
        Exception.__init__(self, msg)
        self.code = code

class Client(object):
    def __init__(self, url=None, user=None, cert=None, act_as=None,
                 ca_certs=None, timeout=None):
        self._url = url or DEFAULT_URL
        self._user = user
        self._cert = cert
        self._oauth = None
        self._actas = act_as or self._user
        self._connection = None
        self._ca_certs = ca_certs
        self._timeout = timeout

    def apply_arcconfig(self, config):
        self._url = config.get('conduit_uri', DEFAULT_URL)
        if self._url == 'https://phabricator.fb.com/api/':
            self._url = 'https://phabricator.intern.facebook.com/api/'
        try:
            hostconfig = config['hosts'][self._url]
            self._user = hostconfig['user']
            if 'oauth' in hostconfig:
                self._oauth = hostconfig['oauth']
            else:
                self._cert = hostconfig['cert']
        except KeyError:
            try:
                hostconfig = config['hosts'][config['hosts'].keys()[0]]
                self._user = hostconfig['user']
                if 'oauth' in hostconfig:
                    self._oauth = hostconfig['oauth']
                else:
                    self._cert = hostconfig['cert']
            except KeyError:
                raise arcconfig.ArcConfigError(
                    'arcrc is missing user '
                    'credentials for host %s.  use '
                    '"arc install-certificate" to fix.' % self._url)
        self._actas = self._user
        self._connection = None

    def call(self, method, args, timeout=None):
        if timeout is None:
            if self._timeout is None:
                timeout = DEFAULT_TIMEOUT
            else:
                timeout = self._timeout
        args['__conduit__'] = {
            'authUser': self._user,
            'actAsUser': self._actas,
            'caller': 'hg',
        }
        if  self._oauth is not None:
            args['__conduit__']['accessToken'] = self._oauth
        else:
            token = '%d' % time.time()
            sig = token + self._cert
            args['__conduit__'].update({
                'authToken': token,
                'authSignature': hashlib.sha1(sig.encode('utf-8')).hexdigest()
            })
        req_data = {
                'params': json.dumps(args),
                'output': 'json',
        }
        headers = (
            ('Connection', 'Keep-Alive'),
        )
        url = self._url + method

        if self._connection is None:
            self._connection = urllib3.PoolManager(ca_certs=self._ca_certs)
        try:
            with warnings.catch_warnings():
                if not self._ca_certs:
                    # ignore the urllib3 certificate verification warnings
                    warnings.simplefilter(
                        'ignore', urllib3.exceptions.InsecureRequestWarning)
                response = self._connection.request(
                    'POST', url, headers=headers, fields=req_data,
                    timeout=timeout)
        except urllib3.exceptions.HTTPError as ex:
            errno = -1
            if ex.args and util.safehasattr(ex.args[0], 'errno'):
                errno = ex.args[0].errno
            raise ClientError(errno, str(ex))

        try:
            response = json.loads(response.data)
        except ValueError:
            # Can't decode the data, not valid JSON (html error page perhaps?)
            raise ClientError(-1, 'did not receive a valid JSON response')

        if response['error_code'] is not None:
            raise ClientError(response['error_code'], response['error_info'])
        return response['result']

class MockClient(object):
    def __init__(self, **kwargs):
        pass

    def apply_arcconfig(self, config):
        pass

    def call(self, method, args, timeout=DEFAULT_TIMEOUT):
        global mocked_responses

        cmd = json.dumps([method, args], sort_keys=True)
        try:
            response = mocked_responses.pop(0)
            # Check expectations via a deep compare of the json representation.
            # We need this because child objects and values are compared by
            # address rather than value.
            expect = json.dumps(response.get('cmd', None), sort_keys=True)
            if cmd != expect:
                raise ClientError(None,
                                  'mock mismatch got %s expected %s' % (
                                  cmd, expect))
            if 'error_info' in response:
                raise ClientError(response.get('error_code', None),
                                  response['error_info'])
            return response['result']
        except IndexError:
            raise ClientError(None,
                  'No more mocked responses available for call to %s' % cmd)


if 'HG_ARC_CONDUIT_MOCK' in os.environ:
    # To facilitate testing, we replace the client object with this
    # fake implementation that returns responses from a file that
    # contains a series of json serialized object values.
    with open(os.environ['HG_ARC_CONDUIT_MOCK'], 'r') as f:
        mocked_responses = json.load(f)
        Client = MockClient

class ClientCache(object):
    def __init__(self):
        self.max_idle_seconds = 10
        self.client = {}
        self.lastuse = {}

    @contextlib.contextmanager
    def getclient(self, ca_certs=None):
        # Use the existing client if we have one and it hasn't been idle too
        # long.
        #
        # We reconnect if we have been idle for too long just in case the
        # server might have closed our connection while we were idle.  (We
        # could potentially check the socket for readability, but that might
        # still race with the server currently closing our socket.)
        client = self.client.get(ca_certs),
        lastuse = self.lastuse.get(ca_certs, 0)
        if client and time.time() <= (lastuse + self.max_idle_seconds):
            # Remove self.client for this ca_certs config while we are using
            # it. If our caller throws an exception during the yield this
            # ensures that we do not continue to use this client later.
            del self.client.pop[ca_certs], self.lastuse[ca_certs]
        else:
            # We have to make a new connection
            client = Client(ca_certs=ca_certs)
            client.apply_arcconfig(arcconfig.load_for_path(os.getcwd()))

        yield client

        # Our caller used this client successfully and did not throw an
        # exception.  Store it to use again next time getclient() is called.
        self.lastuse[ca_certs] = time.time()
        self.client[ca_certs] = client

_clientcache = ClientCache()

def call_conduit(method, args, ca_certs=None, timeout=DEFAULT_TIMEOUT):
    with _clientcache.getclient(ca_certs=ca_certs) as client:
        return client.call(method, args, timeout=timeout)

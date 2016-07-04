# conduit.py
#
# A library function to call a phabricator conduit RPC.
# It's different from fbconduit in that this is an authenticated
# conduit client.

import hashlib
from mercurial.util import httplib

import json
import os
import sys
import time
from mercurial import util
import arcconfig

urlparse = util.urlparse

DEFAULT_HOST = 'https://phabricator.fb.com/api/'
DEFAULT_TIMEOUT = 60
mocked_responses = None

class ClientError(Exception):
    def __init__(self, code, msg):
        Exception.__init__(self, msg)
        self.code = code

class Client(object):
    def __init__(self, host=None, user=None, cert=None, act_as=None):
        self._host = host or DEFAULT_HOST
        self._user = user
        self._cert = cert
        self._actas = act_as or self._user
        self._connection = None

    def apply_arcconfig(self, config):
        self._host = config.get('conduit_uri', DEFAULT_HOST)
        hostconfig = config['hosts'][self._host]
        self._user = hostconfig['user']
        self._cert = hostconfig['cert']
        self._actas = self._user
        self._connection = None

    def call(self, method, args, timeout=DEFAULT_TIMEOUT):
        token = '%d' % time.time()
        sig = token + self._cert
        args['__conduit__'] = {
            'authUser': self._user,
            'actAsUser': self._actas,
            'authToken': token,
            'authSignature': hashlib.sha1(sig.encode('utf-8')).hexdigest(),
        }
        req_data = util.urlreq.urlencode(
            {
                'params': json.dumps(args),
                'output': 'json',
            }
        )
        urlparts = urlparse.urlparse(self._host)
        if self._connection is None:
            if urlparts.scheme == 'http':
                self._connection = httplib.HTTPConnection(
                    urlparts.netloc, timeout=timeout)
            elif urlparts.scheme == 'https':
                self._connection = httplib.HTTPSConnection(
                    urlparts.netloc, timeout=timeout)
            else:
                raise ClientError(
                    None, 'Unknown host scheme: %s', urlparts.scheme)

        # self._connection.set_debuglevel(1)
        self._connection.request('POST', (urlparts.path + method), req_data,
                                 {'Connection': 'Keep-Alive'})

        response = json.load(self._connection.getresponse())
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

def call_conduit(method, args, timeout=DEFAULT_TIMEOUT):
    client = Client()
    client.apply_arcconfig(arcconfig.load_for_path(os.getcwd()))
    return client.call(method, args, timeout=timeout)


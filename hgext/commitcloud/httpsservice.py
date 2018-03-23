# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import gzip
import json
import os
import ssl
import time
from StringIO import StringIO
import urllib

from mercurial import (
    error,
    util,
)

from . import baseservice

httplib = util.httplib

try:
    xrange
except NameError:
    xrange = range

DEFAULT_TIMEOUT = 60
MAX_CONNECT_RETRIES = 2

class HttpsCommitCloudService(baseservice.BaseService):
    """Commit Cloud Client uses interngraph proxy to communicate with
       Commit Cloud Service
    """
    def __init__(self, ui):
        self.ui = ui
        self.ccht = ui.label('#commitcloud', 'commitcloud.hashtag')
        self.host = ui.config('commitcloud', 'host')

        # optional, but needed for using a sandbox
        self.certs = ui.config('commitcloud', 'certs')

        def raiseconfigerror(msg):
            msg = (
                '%s Invalid commitcloud configuration: %s\n'
                'Please, contact the Source Control Team' % (self.ccht, msg))
            raise error.Abort(msg)

        def getauthparams():
            oauth = ui.configbool('commitcloud', 'oauth')
            """ If OAuth authentication is enabled we require
                user specific unique token
                a token can be self-granted at
                https://our.intern.facebook.com/intern/oauth/
                This is the preferred way!
                Currently, we require it in the config
                but later we will make sure
                it's stored securely in a keychain or a file
            """
            if oauth:
                user_token = ui.config('commitcloud', 'user_token')

                if not user_token:
                    raiseconfigerror('user_token is required')

                ui.debug(
                    '%s OAuth based authentication is used\n'
                    % self.ccht)

                return {'access_token': user_token}
            else:
                """ If app-based authentication is used
                    app id and secret token (app specific) are required
                    (this was is for testing purposes, to be deprecated)
                """
                app_id = ui.config('commitcloud', 'app_id')
                app_access_token = ui.config('commitcloud', 'app_token')

                if not app_access_token or not app_id:
                    raiseconfigerror('app_id and app_token are required')

                ui.debug(
                    '%s app-based authentication is used\n'
                    % self.ccht)

                return {
                    'app': app_id,
                    'token': app_access_token,
                }

        self.auth_params = urllib.urlencode(getauthparams())

        # we have control on compression here
        # on both client side and server side compression
        self.headers = {
            'Connection': 'Keep-Alive',
            'Content-Type': 'application/binary',
            'Accept-encoding': 'none, gzip',
            'Content-Encoding': 'gzip',
        }
        self.connection = httplib.HTTPSConnection(
            self.host,
            context=ssl.create_default_context(cafile=self.certs)
                if self.certs else ssl.create_default_context(),
            timeout=DEFAULT_TIMEOUT
        )

        if not self.host:
            raiseconfigerror('host is required')

        reponame = ui.config('paths', 'default')
        if reponame:
            self.repo_name = os.path.basename(reponame)
        else:
            raiseconfigerror('unknown repo')

        self.workspace = ui.username()
        ui.warn(('%s enabled for workspace \'%s\'\n' % (self.ccht,
            self.workspace)))

    def _getheader(self, s):
        return self.headers.get(s)

    def _send(self, path, data):
        e = None
        rdata = None
        if self._getheader('Content-Encoding') == 'gzip':
            buffer = StringIO()
            with gzip.GzipFile(fileobj=buffer, mode='w') as compressed:
                compressed.write(json.dumps(data))
                compressed.flush()
            rdata = buffer.getvalue()
        else:
            rdata = json.dumps(data)

        # exponential backoff here on failure, 1s, 2s, 4s, 8s, 16s etc
        sl = 1
        for attempt in xrange(MAX_CONNECT_RETRIES):
            try:
                self.connection.request('POST', path, rdata, self.headers)
                resp = self.connection.getresponse()
                if resp.status != 200:
                    raise error.Abort(resp.reason)
                if resp.getheader('Content-Encoding') == 'gzip':
                    resp = gzip.GzipFile(fileobj=StringIO(resp.read()))
                return json.load(resp)
            except httplib.HTTPException as e:
                self.connection.connect()
            time.sleep(sl)
            sl *= 2
        if e:
            raise error.Abort(str(e))

    def getreferences(self, baseversion):
        self.ui.debug("%s sending 'get_references' request\n" % self.ccht)

        # send request
        path = '/commit_cloud/get_references?' + self.auth_params
        data = {
            'base_version': baseversion,
            'repo_name': self.repo_name,
            'workspace': self.workspace,
        }
        start = time.time()
        response = self._send(path, data)
        elapsed = time.time() - start
        self.ui.debug("%s responce received in %0.2f sec\n" % (
            self.ccht, elapsed)
        )

        if 'error' in response:
            raise error.Abort(self.ccht + ' ' + response['error'])

        version = response['ref']['version']

        if version == 0:
            self.ui.warn((
                '%s \'get_references\' '
                'informs the workspace \'%s\' is not known by server\n'
                % (self.ccht, self.workspace)))
            return baseservice.References(version, None, None, None)

        if version == baseversion:
            self.ui.debug(
                '%s \'get_references\' '
                'confirms the current version %s is the latest\n'
                % (self.ccht, version))
            return baseservice.References(version, None, None, None)

        self.ui.debug(
            '%s \'get_references\' '
            'returns version %s, current version %s\n'
            % (self.ccht, version, baseversion))
        return self._makereferences(response['ref'])

    def updatereferences(self, version, oldheads, newheads, oldbookmarks,
                          newbookmarks, newobsmarkers):
        self.ui.debug("%s sending 'update_references' request\n" % self.ccht)

        # send request
        path = '/commit_cloud/update_references?' + self.auth_params

        data = {
            'version': version,
            'repo_name': self.repo_name,
            'workspace': self.workspace,
            'removed_heads': oldheads,
            'new_heads': newheads,
            'removed_bookmarks': oldbookmarks,
            'updated_bookmarks': newbookmarks,
            'new_obsmarkers': self._encodedmarkers(newobsmarkers),
        }

        start = time.time()
        response = self._send(path, data)
        elapsed = time.time() - start
        self.ui.debug("%s responce received in %0.2f sec\n" % (
            self.ccht, elapsed)
        )

        if 'error' in response:
            raise error.Abort(self.ccht + ' ' + response['error'])

        data = response['ref']
        rc = response['rc']
        newversion = data['version']

        if rc != 0:
            self.ui.debug(
                '%s \'update_references\' '
                'rejected update, current version %d is old, '
                'client needs to sync to version %d first\n'
                % (self.ccht, version, newversion))
            return False, self._makereferences(data)

        self.ui.debug(
            '%s \'update_references\' '
            'accepted update, old version is %d, new version is %d\n'
            % (self.ccht, version, newversion))

        return True, baseservice.References(newversion, None, None, None)

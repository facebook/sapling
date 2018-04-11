# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

# Standard Library
import gzip
import json
import ssl
import time

# Mercurial
from mercurial.i18n import _
from mercurial import util

from . import (
    baseservice,
    commitcloudcommon,
    commitcloudutil,
)

httplib = util.httplib
highlightdebug = commitcloudcommon.highlightdebug
highlightstatus = commitcloudcommon.highlightstatus

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

    def __init__(self, ui, repo):
        self.ui = ui
        self.repo = repo
        self.host = ui.config('commitcloud', 'host')

        # optional, but needed for using a sandbox
        self.certs = ui.config('commitcloud', 'certs')

        # debug option
        self.debugrequests = ui.config('commitcloud', 'debugrequests')

        def getauthparams():
            oauth = ui.configbool('commitcloud', 'oauth')
            """ If OAuth authentication is enabled we require
                user specific unique token
            """
            if oauth:
                user_token = commitcloudutil.TokenLocator(self.ui).token
                if not user_token:
                    raise commitcloudcommon.RegistrationError(
                        ui, _('valid user token is required'))

                highlightdebug(self.ui, 'OAuth based authentication is used\n')
                return {'access_token': user_token}
            else:
                """ If app-based authentication is used
                    app id and secret token (app specific) are required
                    (this was is for testing purposes, to be deprecated)
                """
                app_id = ui.config('commitcloud', 'app_id')
                app_access_token = ui.config('commitcloud', 'app_token')

                if not app_access_token or not app_id:
                    raise commitcloudcommon.ConfigurationError(
                        self.ui, _('app_id and app_token are required'))

                highlightdebug(self.ui, 'app-based authentication is used\n')
                return {
                    'app': app_id,
                    'token': app_access_token,
                }

        self.auth_params = util.urlreq.urlencode(getauthparams())

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
            raise commitcloudcommon.ConfigurationError(
                self.ui, _('host is required'))

        workspacemanager = commitcloudutil.WorkspaceManager(self.repo)
        self.repo_name = workspacemanager.reponame

        if not self.repo_name:
            raise commitcloudcommon.ConfigurationError(
                self.ui, _('unknown repo'))

        self.workspace = workspacemanager.workspace
        if not self.workspace:
            raise commitcloudcommon.WorkspaceError(
                self.ui, _('undefined workspace'))

        self.ui.status(
            _("current workspace is '%s'\n") %
            self.workspace)

    def _getheader(self, s):
        return self.headers.get(s)

    def _send(self, path, data):
        e = None
        rdata = None
        # print all requests if debugrequests and debug are both on
        if self.debugrequests:
            self.ui.debug('%s\n' % json.dumps(data, indent=4))
        if self._getheader('Content-Encoding') == 'gzip':
            buffer = util.stringio()
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
                if resp.status == 401:
                    raise commitcloudcommon.RegistrationError(self.ui,
                        _('unauthorized client (token is invalid)'))
                if resp.status != 200:
                    raise commitcloudcommon.ServiceError(self.ui, resp.reason)
                if resp.getheader('Content-Encoding') == 'gzip':
                    resp = gzip.GzipFile(fileobj=util.stringio(resp.read()))
                return json.load(resp)
            except httplib.HTTPException as e:
                self.connection.connect()
            time.sleep(sl)
            sl *= 2
        if e:
            raise commitcloudcommon.ServiceError(self.ui, str(e))

    def getreferences(self, baseversion):
        highlightdebug(self.ui, "sending 'get_references' request\n")

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
        highlightdebug(self.ui, "responce received in %0.2f sec\n" % elapsed)

        if 'error' in response:
            raise commitcloudcommon.ServiceError(self.ui, response['error'])

        version = response['ref']['version']

        if version == 0:
            highlightstatus(self.ui, _(
                "'get_references' "
                "informs the workspace '%s' is not known by server\n")
                % self.workspace)
            return baseservice.References(version, None, None, None)

        if version == baseversion:
            highlightdebug(self.ui,
                           "'get_references' "
                           'confirms the current version %s is the latest\n'
                           % version)
            return baseservice.References(version, None, None, None)

        highlightdebug(self.ui,
                       "'get_references' "
                       'returns version %s, current version %s\n'
                       % (version, baseversion))
        return self._makereferences(response['ref'])

    def updatereferences(self, version, oldheads, newheads, oldbookmarks,
                         newbookmarks, newobsmarkers):
        highlightdebug(self.ui, "sending 'update_references' request\n")

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
        highlightdebug(self.ui, "responce received in %0.2f sec\n" % elapsed)

        if 'error' in response:
            raise commitcloudcommon.ServiceError(self.ui, response['error'])

        data = response['ref']
        rc = response['rc']
        newversion = data['version']

        if rc != 0:
            highlightdebug(self.ui,
                           "'update_references' "
                           'rejected update, current version %d is old, '
                           'client needs to sync to version %d first\n'
                           % (version, newversion))
            return False, self._makereferences(data)

        highlightdebug(
            self.ui, "'update_references' "
            'accepted update, old version is %d, new version is %d\n' %
            (version, newversion))

        return True, baseservice.References(newversion, None, None, None)

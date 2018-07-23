# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

# Standard Library
import gzip
import json
import socket
import ssl
import time

from mercurial import error, util
from mercurial.i18n import _

from . import baseservice, commitcloudcommon


httplib = util.httplib
highlightdebug = commitcloudcommon.highlightdebug
highlightstatus = commitcloudcommon.highlightstatus

try:
    xrange
except NameError:
    xrange = range

# clean up helper (to use with json.dumps)
# filter out the fields with None and empty arrays / maps


def cleandict(d):
    if not isinstance(d, dict):
        return d
    return dict(
        (k, cleandict(v))
        for k, v in d.iteritems()
        if (v is not None and not (util.safehasattr(v, "__len__") and len(v) == 0))
    )


DEFAULT_TIMEOUT = 60
MAX_CONNECT_RETRIES = 2


class HttpsCommitCloudService(baseservice.BaseService):
    """Commit Cloud Client uses http endpoint to communicate with
       Commit Cloud Service
    """

    def __init__(self, ui, token=None):
        self.ui = ui
        self.token = token
        if token is None:
            raise commitcloudcommon.RegistrationError(
                ui, _("valid user token is required")
            )

        self.remote_host = ui.config("commitcloud", "remote_host")
        if not self.remote_host:
            raise commitcloudcommon.ConfigurationError(
                self.ui, _("remote_host is required")
            )

        # optional, but needed for using a sandbox
        self.certs = ui.config("commitcloud", "certs")

        # debug option
        self.debugrequests = ui.config("commitcloud", "debugrequests")

        # we have control on compression here
        # on both client side and server side compression
        self.headers = {
            "Connection": "Keep-Alive",
            "Content-Type": "application/binary",
            "Accept-encoding": "none, gzip",
            "Content-Encoding": "gzip",
            "Authorization": "OAuth %s" % token,
        }
        self.connection = httplib.HTTPSConnection(
            self.remote_host,
            context=ssl.create_default_context(cafile=self.certs)
            if self.certs
            else ssl.create_default_context(),
            timeout=DEFAULT_TIMEOUT,
        )

    def requiresauthentication(self):
        return True

    def _getheader(self, s):
        return self.headers.get(s)

    def _send(self, path, data):
        e = None
        rdata = None
        # print request if debugrequests and debug are both on
        if self.debugrequests:
            self.ui.debug("%s\n" % json.dumps(cleandict(data), indent=4))
        if self._getheader("Content-Encoding") == "gzip":
            buffer = util.stringio()
            with gzip.GzipFile(fileobj=buffer, mode="w") as compressed:
                compressed.write(json.dumps(data))
                compressed.flush()
            rdata = buffer.getvalue()
        else:
            rdata = json.dumps(data)

        # exponential backoff here on failure, 1s, 2s, 4s, 8s, 16s etc
        sl = 1
        for attempt in xrange(MAX_CONNECT_RETRIES):
            try:
                self.connection.request("POST", path, rdata, self.headers)
                resp = self.connection.getresponse()
                if resp.status == 401:
                    raise commitcloudcommon.RegistrationError(
                        self.ui, _("unauthorized client (token is invalid)")
                    )
                if resp.status != 200:
                    raise commitcloudcommon.ServiceError(self.ui, resp.reason)
                if resp.getheader("Content-Encoding") == "gzip":
                    resp = gzip.GzipFile(fileobj=util.stringio(resp.read()))
                data = json.load(resp)
                # print response if debugrequests and debug are both on
                if self.debugrequests:
                    self.ui.debug("%s\n" % json.dumps(cleandict(data), indent=4))
                return data
            except httplib.HTTPException as e:
                self.connection.connect()
            except (socket.timeout, socket.gaierror) as e:
                raise error.Abort(
                    _("network error: %s") % e, hint=_("check your network connection")
                )
            time.sleep(sl)
            sl *= 2
        if e:
            raise commitcloudcommon.ServiceError(self.ui, str(e))

    def check(self):
        # send a check request.  Currently this is an empty 'get_references'
        # request, which asks for the latest version of workspace '' for repo
        # ''.  That always returns a valid response indicating there is no
        # workspace with that name for that repo.
        # TODO: Make this a dedicated request
        path = "/commit_cloud/get_references"
        response = self._send(path, {})
        if "error" in response:
            raise commitcloudcommon.ServiceError(self.ui, response["error"])

    def getreferences(self, reponame, workspace, baseversion):
        highlightdebug(self.ui, "sending 'get_references' request\n")

        # send request
        path = "/commit_cloud/get_references"
        data = {
            "base_version": baseversion,
            "repo_name": reponame,
            "workspace": workspace,
        }
        start = time.time()
        response = self._send(path, data)
        elapsed = time.time() - start
        highlightdebug(self.ui, "responce received in %0.2f sec\n" % elapsed)

        if "error" in response:
            raise commitcloudcommon.ServiceError(self.ui, response["error"])

        version = response["ref"]["version"]

        if version == 0:
            highlightdebug(
                self.ui,
                _(
                    "'get_references' "
                    "returns that workspace '%s' is not known by server\n"
                )
                % workspace,
            )
            return baseservice.References(version, None, None, None)

        if version == baseversion:
            highlightdebug(
                self.ui,
                "'get_references' "
                "confirms the current version %s is the latest\n" % version,
            )
            return baseservice.References(version, None, None, None)

        highlightdebug(
            self.ui,
            "'get_references' "
            "returns version %s, current version %s\n" % (version, baseversion),
        )
        return self._makereferences(response["ref"])

    def updatereferences(
        self,
        reponame,
        workspace,
        version,
        oldheads,
        newheads,
        oldbookmarks,
        newbookmarks,
        newobsmarkers,
    ):
        highlightdebug(self.ui, "sending 'update_references' request\n")

        # remove duplicates, must preserve order in the newheads list
        newheadsset = set(newheads)
        commonset = set([item for item in oldheads if item in newheadsset])

        newheads = filter(lambda h: h not in commonset, newheads)
        oldheads = filter(lambda h: h not in commonset, oldheads)

        # send request
        path = "/commit_cloud/update_references"

        data = {
            "version": version,
            "repo_name": reponame,
            "workspace": workspace,
            "removed_heads": oldheads,
            "new_heads": newheads,
            "removed_bookmarks": oldbookmarks,
            "updated_bookmarks": newbookmarks,
            "new_obsmarkers_data": self._encodedmarkers(newobsmarkers),
        }

        start = time.time()
        response = self._send(path, data)
        elapsed = time.time() - start
        highlightdebug(self.ui, "responce received in %0.2f sec\n" % elapsed)

        if "error" in response:
            raise commitcloudcommon.ServiceError(self.ui, response["error"])

        data = response["ref"]
        rc = response["rc"]
        newversion = data["version"]

        if rc != 0:
            highlightdebug(
                self.ui,
                "'update_references' "
                "rejected update, current version %d is old, "
                "client needs to sync to version %d first\n" % (version, newversion),
            )
            return False, self._makereferences(data)

        highlightdebug(
            self.ui,
            "'update_references' "
            "accepted update, old version is %d, new version is %d\n"
            % (version, newversion),
        )

        return True, baseservice.References(newversion, None, None, None)

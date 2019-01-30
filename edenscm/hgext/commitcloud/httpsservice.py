# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

# Standard Library
import gzip
import json
import os
import socket
import ssl
import tempfile
import time
from multiprocessing.pool import ThreadPool
from subprocess import PIPE, Popen

from edenscm.mercurial import error, util
from edenscm.mercurial.i18n import _

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


DEFAULT_TIMEOUT = 180
MAX_CONNECT_RETRIES = 2


class HttpsCommitCloudService(baseservice.BaseService):
    """Commit Cloud Client uses http endpoint to communicate with
       Commit Cloud Service
    """

    def __init__(self, ui, token=None):
        self.ui = ui
        self.token = token
        self.debugrequests = ui.config("commitcloud", "debugrequests")
        self.remote_host = ui.config("commitcloud", "remote_host")
        self.remote_port = ui.configint("commitcloud", "remote_port")
        self.client_certs = util.expanduserpath(
            ui.config("commitcloud", "tls.client_certs")
        )
        self.ca_certs = util.expanduserpath(ui.config("commitcloud", "tls.ca_certs"))
        self.check_hostname = ui.configbool("commitcloud", "tls.check_hostname")

        # validation
        if not self.remote_host:
            raise commitcloudcommon.ConfigurationError(
                self.ui, _("'remote_host' is required")
            )

        if self.client_certs and not os.path.isfile(self.client_certs):
            raise commitcloudcommon.ConfigurationError(
                ui,
                _("tls.ca_certs resolved to '%s' (no such file or is a directory)")
                % self.client_certs,
            )

        if self.ca_certs and not os.path.isfile(self.ca_certs):
            raise commitcloudcommon.ConfigurationError(
                ui,
                _("tls.ca_certs resolved to '%s' (no such file or is a directory)")
                % self.ca_certs,
            )

        self._setuphttpsconnection()

    def _setuphttpsconnection(self):
        # setting up HTTS connection

        # enable client side compression
        # data in the response is also requested compressed
        self.headers = {
            "Connection": "keep-alive",
            "Content-Type": "application/json",
            "Accept-Encoding": "none, gzip",
            "Content-Encoding": "gzip",
        }
        if self.token:
            self.headers["Authorization"] = "OAuth %s" % self.token
        sslcontext = ssl.create_default_context()
        if self.client_certs:
            sslcontext.load_cert_chain(self.client_certs)
        if self.ca_certs:
            sslcontext.load_verify_locations(self.ca_certs)

        try:
            sslcontext.check_hostname = self.check_hostname
            self.connection = httplib.HTTPSConnection(
                self.remote_host,
                self.remote_port,
                context=sslcontext,
                timeout=DEFAULT_TIMEOUT,
                check_hostname=self.check_hostname,
            )
        except TypeError:
            sslcontext.check_hostname = self.check_hostname
            self.connection = httplib.HTTPSConnection(
                self.remote_host,
                self.remote_port,
                context=sslcontext,
                timeout=DEFAULT_TIMEOUT,
            )
        highlightdebug(
            self.ui,
            "will be connecting to %s:%d\n" % (self.remote_host, self.remote_port),
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

        def _tlserror(e):
            # build tls error with all configuration details
            details = []
            if self.ca_certs:
                details.append(
                    _(
                        "* certificate authority (CA) file used '%s' (config option commitcloud.tls.ca_certs)"
                    )
                    % self.ca_certs
                )
            if self.client_certs:
                details.append(
                    _(
                        "* client cert file used '%s' (config option commitcloud.tls.client_certs)"
                    )
                    % self.client_certs
                )
            if self.check_hostname:
                details.append(
                    _(
                        "* tls hostname validation is enabled (config option commitcloud.tls.check_hostname)"
                    )
                )
            return commitcloudcommon.TLSAccessError(self.ui, str(e), details)

        for attempt in xrange(MAX_CONNECT_RETRIES):
            try:
                self.connection.request("POST", path, rdata, self.headers)
                resp = self.connection.getresponse()
                if resp.status == httplib.UNAUTHORIZED:
                    raise commitcloudcommon.RegistrationError(
                        self.ui, _("unauthorized client (token is invalid)")
                    )
                if resp.status != httplib.OK:
                    raise commitcloudcommon.ServiceError(
                        self.ui, "%d %s" % (resp.status, resp.reason)
                    )
                if resp.getheader("Content-Encoding") == "gzip":
                    resp = gzip.GzipFile(fileobj=util.stringio(resp.read()))
                data = json.load(resp)
                # print response if debugrequests and debug are both on
                if self.debugrequests:
                    self.ui.debug("%s\n" % json.dumps(cleandict(data), indent=4))
                return data
            except httplib.HTTPException as e:
                self.connection.close()
                self.connection.connect()
            except (socket.timeout, socket.gaierror) as e:
                raise error.Abort(
                    _("network error: %s") % e, hint=_("check your network connection")
                )
            except socket.error as e:
                if "SSL" in str(e):
                    raise _tlserror(e)
                raise commitcloudcommon.ServiceError(self.ui, str(e))
            except ssl.CertificateError as e:
                raise _tlserror(e)
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

        highlightdebug(
            self.ui, "sending empty 'get_references' request to check authentication\n"
        )
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
        highlightdebug(self.ui, "response received in %0.2f sec\n" % elapsed)

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
            return baseservice.References(version, None, None, None, None)

        if version == baseversion:
            highlightdebug(
                self.ui,
                "'get_references' "
                "confirms the current version %s is the latest\n" % version,
            )
            return baseservice.References(version, None, None, None, None)

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
        highlightdebug(self.ui, "response received in %0.2f sec\n" % elapsed)

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

        return True, baseservice.References(newversion, None, None, None, None)

    def getsmartlog(self, reponame, workspace, repo):

        highlightdebug(self.ui, "sending 'get_smartlog' request\n")

        path = "/commit_cloud/get_smartlog"
        data = {"repo_name": reponame, "workspace": workspace}

        start = time.time()
        response = self._send(path, data)
        elapsed = time.time() - start
        highlightdebug(self.ui, "responce received in %0.2f sec\n" % elapsed)

        if "error" in response:
            raise commitcloudcommon.ServiceError(self.ui, response["error"])

        # if 200 OK response format is:
        # {
        #   "rc":0,
        #   "smartlog": <thrift structure SmartlogData serialized to json using Thrift JSON serialization>
        # }
        smartlog = response["smartlog"]

        highlightdebug(
            self.ui, "'get_smartlog' returns %d entries\n" % len(smartlog["nodes"])
        )

        nodes = self._makenodes(smartlog)
        try:
            return self._makefakedag(nodes, repo)
        except Exception as e:
            raise commitcloudcommon.UnexpectedError(self.ui, e)

    def _getbundleshandles(self, reponame, heads):
        # do not send empty list
        if not heads:
            return heads

        highlightdebug(self.ui, "sending 'get_bundles_handles' request\n")

        # send request
        path = "/commit_cloud/get_bundles_handles"

        data = {"repo_name": reponame, "heads": heads}

        start = time.time()
        response = self._send(path, data)
        elapsed = time.time() - start
        highlightdebug(self.ui, "response received in %0.2f sec\n" % elapsed)

        if "error" in response:
            raise commitcloudcommon.ServiceError(self.ui, response["error"])

        return response["data"]["handles"]

    def filterpushedheads(self, reponame, heads):
        """Filter heads that have already been pushed to Commit Cloud backend

        Current way to filter that is to check bundles on the server side
        """
        notbackeduphandles = set(
            [i for i, s in enumerate(self._getbundleshandles(reponame, heads)) if not s]
        )

        highlightdebug(
            self.ui, "%d heads are not backed up\n" % len(notbackeduphandles)
        )

        return [h for i, h in enumerate(heads) if i in notbackeduphandles]

    def getbundles(self, reponame, heads, unbundlefn):
        """Downloading and applying mercurial bundles directly
        """
        command = self.ui.config("commitcloud", "get_command")
        handles = self._getbundleshandles(reponame, heads)
        if not all(handles):
            raise error.Abort(_("some bundles are missing in the bundle store"))

        def downloader(param):
            head = param[0]
            handle = param[1]
            highlightstatus(
                self.ui,
                _("downloading mercurial bundle '%s' for changeset '%s'\n")
                % (handle, head),
            )
            tempdir = tempfile.mkdtemp()
            dstfile = os.path.join(tempdir, handle.lower())
            util.tryunlink(dstfile)
            fullcommand = command.format(filename=dstfile, handle=handle)
            p = Popen(
                fullcommand,
                close_fds=util.closefds,
                stdout=PIPE,
                stderr=PIPE,
                stdin=open(os.devnull, "r"),
                shell=True,
            )
            stdout, stderr = p.communicate()
            rc = p.returncode
            if rc != 0:
                if not stderr:
                    stderr = stdout
                raise commitcloudcommon.SubprocessError(self.ui, rc, stderr)
            return dstfile

        files = []
        try:
            pool = ThreadPool(8)
            seen = set()
            handles = [
                seen.add(handle) or (heads[i], handle)
                for i, handle in enumerate(handles)
                if handle not in seen
            ]
            files = pool.map(downloader, handles)
            unbundlefn(files)
        finally:
            for dstfile in files:
                util.tryunlink(dstfile)

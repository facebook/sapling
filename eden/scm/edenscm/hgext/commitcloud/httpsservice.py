# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

# Standard Library
import gzip
import os
import socket
import ssl
import time

from edenscm.mercurial import error, json, perftrace, pycompat, util
from edenscm.mercurial.i18n import _

from . import baseservice, error as ccerror


# pyre-fixme[11]: Annotation `client` is not defined as a type.
httplib = util.httplib

try:
    range
except NameError:
    range = range

# clean up helper (to use with json.dumps)
# filter out the fields with None and empty arrays / maps


def cleandict(d):
    if not isinstance(d, dict):
        return d
    return dict(
        (k, cleandict(v))
        for k, v in pycompat.iteritems(d)
        if (v is not None and not (util.safehasattr(v, "__len__") and len(v) == 0))
    )


DEFAULT_TIMEOUT = 180
MAX_CONNECT_RETRIES = 2


class _HttpsCommitCloudService(baseservice.BaseService):
    """Commit Cloud Client uses http endpoint to communicate with
       the commit cloud service
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
            raise ccerror.ConfigurationError(self.ui, _("'remote_host' is required"))

        if self.client_certs and not os.path.isfile(self.client_certs):
            raise ccerror.TLSConfigurationError(
                ui, _("%s (no such file or is a directory)") % self.client_certs
            )

        if self.ca_certs and not os.path.isfile(self.ca_certs):
            raise ccerror.TLSConfigurationError(
                ui, _("%s (no such file or is a directory)") % self.ca_certs
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
        self.ui.debug(
            "will be connecting to %s:%d\n" % (self.remote_host, self.remote_port),
            component="commitcloud",
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
            self.ui.debug("%s\n" % json.dumps(cleandict(data)))
        if self._getheader("Content-Encoding") == "gzip":
            buffer = util.stringio()
            with gzip.GzipFile(fileobj=buffer, mode="w") as compressed:
                compressed.write(pycompat.encodeutf8(json.dumps(data)))
                compressed.flush()
            rdata = buffer.getvalue()
        else:
            rdata = pycompat.encodeutf8(json.dumps(data))

        # exponential backoff here on failure, 1s, 2s, 4s, 8s, 16s etc
        sl = 1

        for attempt in range(MAX_CONNECT_RETRIES):
            try:
                self.connection.request("POST", path, rdata, self.headers)
                resp = self.connection.getresponse()
                if resp.status == httplib.UNAUTHORIZED:
                    raise ccerror.RegistrationError(self.ui, _("unauthorized client"))
                if resp.status == httplib.FORBIDDEN:
                    raise ccerror.RegistrationError(self.ui, _("forbidden client"))
                if resp.status == httplib.BAD_REQUEST:
                    raise ccerror.BadRequestError(self.ui, resp.reason)
                if resp.status != httplib.OK:
                    raise ccerror.ServiceError(
                        self.ui, "%d %s" % (resp.status, resp.reason)
                    )
                if resp.getheader("Content-Encoding") == "gzip":
                    resp = gzip.GzipFile(fileobj=util.stringio(resp.read()))
                data = json.load(resp)
                # print response if debugrequests and debug are both on
                if self.debugrequests:
                    self.ui.debug("%s\n" % json.dumps(cleandict(data)))
                return data
            except httplib.HTTPException:
                self.connection.close()
                self.connection.connect()
            except (socket.timeout, socket.gaierror) as e:
                raise error.Abort(
                    _("network error: %s") % e, hint=_("check your network connection")
                )
            except socket.error as e:
                if "SSL" in str(e):
                    raise ccerror.TLSAccessError(self.ui, str(e))
                raise ccerror.ServiceError(self.ui, str(e))
            except ssl.CertificateError as e:
                raise ccerror.TLSAccessError(self.ui, str(e))
            time.sleep(sl)
            sl *= 2
        if e:
            raise ccerror.ServiceError(self.ui, str(e))

    @perftrace.tracefunc("Check Commit Cloud Authentication")
    def check(self):
        # send a check request.  Currently this is an empty 'get_references'
        # request, which asks for the latest version of workspace '' for repo
        # ''.  That always returns a valid response indicating there is no
        # workspace with that name for that repo.
        # TODO: Make this a dedicated request

        self.ui.debug(
            "sending empty 'get_references' request to check authentication\n",
            component="commitcloud",
        )
        path = "/commit_cloud/get_references"
        response = self._send(path, {})
        if "error" in response:
            raise ccerror.ServiceError(self.ui, response["error"])

    @perftrace.tracefunc("Get Commit Cloud References")
    def getreferences(self, reponame, workspace, baseversion):
        self.ui.debug("sending 'get_references' request\n", component="commitcloud")

        # send request
        path = "/commit_cloud/get_references"
        data = {
            "base_version": baseversion,
            "repo_name": reponame,
            "workspace": workspace,
        }
        start = util.timer()
        response = self._send(path, data)
        elapsed = util.timer() - start
        self.ui.debug(
            "response received in %0.2f sec\n" % elapsed, component="commitcloud"
        )

        if "error" in response:
            raise ccerror.ServiceError(self.ui, response["error"])

        version = response["ref"]["version"]

        if version == 0:
            self.ui.debug(
                "'get_references' returns that workspace '%s' is not known by server\n"
                % workspace,
                component="commitcloud",
            )
            return baseservice.References(version, None, None, None, None, None, None)

        if version == baseversion:
            self.ui.debug(
                "'get_references' confirms the current version %s is the latest\n"
                % version,
                component="commitcloud",
            )
            return baseservice.References(version, None, None, None, None, None, None)

        self.ui.debug(
            "'get_references' returns version %s, current version %s\n"
            % (version, baseversion),
            component="commitcloud",
        )
        return self._makereferences(response["ref"])

    @perftrace.tracefunc("Update Commit Cloud References")
    def updatereferences(
        self,
        reponame,
        workspace,
        version,
        oldheads=None,
        newheads=None,
        oldbookmarks=None,
        newbookmarks=None,
        newobsmarkers=None,
        oldremotebookmarks=None,
        newremotebookmarks=None,
        oldsnapshots=None,
        newsnapshots=None,
        logopts={},
    ):
        self.ui.debug("sending 'update_references' request\n", component="commitcloud")
        oldheads = oldheads or []
        newheads = newheads or []
        oldbookmarks = oldbookmarks or []
        newbookmarks = newbookmarks or {}
        newobsmarkers = newobsmarkers or []
        oldremotebookmarks = oldremotebookmarks or []
        newremotebookmarks = newremotebookmarks or {}
        oldsnapshots = oldsnapshots or []
        newsnapshots = newsnapshots or []
        self.ui.log(
            "commitcloud_updates",
            version=version,
            repo=reponame,
            workspace=workspace,
            oldheadcount=len(oldheads),
            newheadcount=len(newheads),
            oldbookmarkcount=len(oldremotebookmarks),
            newbookmarkcount=len(newbookmarks),
            oldremotebookmarkcount=len(oldremotebookmarks),
            newremotebookmarkcount=len(newremotebookmarks),
            **logopts
        )

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
            "removed_remote_bookmarks": self._makeremotebookmarks(oldremotebookmarks),
            "updated_remote_bookmarks": self._makeremotebookmarks(newremotebookmarks),
            "removed_snapshots": oldsnapshots,
            "new_snapshots": newsnapshots,
        }

        start = util.timer()
        response = self._send(path, data)
        elapsed = util.timer() - start
        self.ui.debug(
            "response received in %0.2f sec\n" % elapsed, component="commitcloud"
        )

        if "error" in response:
            raise ccerror.ServiceError(self.ui, response["error"])

        data = response["ref"]
        rc = response["rc"]
        newversion = data["version"]

        if rc != 0:
            self.ui.debug(
                "'update_references' rejected update, current version %d is old, "
                "client needs to sync to version %d first\n" % (version, newversion),
                component="commitcloud",
            )
            return False, self._makereferences(data)

        self.ui.debug(
            "'update_references' accepted update, old version is %d, new version is %d\n"
            % (version, newversion),
            component="commitcloud",
        )

        return (
            True,
            baseservice.References(newversion, None, None, None, None, None, None),
        )

    @perftrace.tracefunc("Get Commit Cloud Smartlog")
    def getsmartlog(self, reponame, workspace, repo, limit, flags=[]):
        self.ui.debug("sending 'get_smartlog' request\n", component="commitcloud")

        path = "/commit_cloud/get_smartlog"
        data = {"repo_name": reponame, "workspace": workspace, "flags": flags}

        start = util.timer()
        response = self._send(path, data)
        elapsed = util.timer() - start
        self.ui.debug(
            "responce received in %0.2f sec\n" % elapsed, component="commitcloud"
        )

        if "error" in response:
            raise ccerror.ServiceError(self.ui, response["error"])

        # if 200 OK response format is:
        # {
        #   "rc":0,
        #   "smartlog": <thrift structure SmartlogData serialized to json using Thrift JSON serialization>
        # }
        smartlog = response["smartlog"]
        if limit != 0:
            cutoff = int(time.time()) - limit
            smartlog["nodes"] = list(
                filter(lambda x: x["date"] >= cutoff, smartlog["nodes"])
            )
        self.ui.debug(
            "'get_smartlog' returns %d entries\n" % len(smartlog["nodes"]),
            component="commitcloud",
        )

        try:
            return self._makesmartloginfo(smartlog)
        except Exception as e:
            raise ccerror.UnexpectedError(self.ui, e)

    @perftrace.tracefunc("Get Commit Cloud Smartlog By Version")
    def getsmartlogbyversion(
        self, reponame, workspace, repo, date, version, limit, flags=[]
    ):
        self.ui.debug("sending 'get_old_smartlog' request\n", component="commitcloud")
        path = "/commit_cloud/get_smartlog_by_version"
        if date:
            data = {
                "repo_name": reponame,
                "workspace": workspace,
                "timestamp": date[0],
                "flags": flags,
            }
        else:
            data = {
                "repo_name": reponame,
                "workspace": workspace,
                "version": version,
                "flags": flags,
            }

        start = util.timer()
        response = self._send(path, data)
        elapsed = util.timer() - start
        self.ui.debug(
            "response received in %0.2f sec\n" % elapsed, component="commitcloud"
        )

        if "error" in response:
            raise ccerror.ServiceError(self.ui, response["error"])

        # if 200 OK response format is:
        # {
        #   "rc":0,
        #   "smartlog": <thrift structure SmartlogData serialized to json using Thrift JSON serialization>
        # }
        smartlog = response["smartlog"]
        if limit != 0:
            cutoff = smartlog["timestamp"] - limit
            smartlog["nodes"] = list(
                filter(lambda x: x["date"] >= cutoff, smartlog["nodes"])
            )

        self.ui.debug(
            "'get_smartlog' returns %d entries\n" % len(smartlog["nodes"]),
            component="commitcloud",
        )

        try:
            return self._makesmartloginfo(smartlog)
        except Exception as e:
            raise ccerror.UnexpectedError(self.ui, e)

    @perftrace.tracefunc("Get list of historical versions")
    def gethistoricalversions(self, reponame, workspace):
        self.ui.debug(
            "sending 'get_historical_versions' request\n", component="commitcloud"
        )
        path = "/commit_cloud/get_historical_versions"
        data = {"repo_name": reponame, "workspace": workspace}
        start = util.timer()
        response = self._send(path, data)
        elapsed = util.timer() - start
        self.ui.debug(
            "response received in %0.2f sec\n" % elapsed, component="commitcloud"
        )

        if "error" in response:
            raise ccerror.ServiceError(self.ui, response["error"])

        versions = response["versions"]["versions"]

        self.ui.debug(
            "'get_historical_versions' returns %d entries\n" % len(versions),
            component="commitcloud",
        )

        try:
            return versions
        except Exception as e:
            raise ccerror.UnexpectedError(self.ui, e)

    @perftrace.tracefunc("update checkout locations")
    def updatecheckoutlocations(
        self, reponame, workspace, hostname, commit, checkoutpath, sharedpath, unixname
    ):
        self.ui.debug(
            "sending 'update_checkout_locations' request\n", component="commitcloud"
        )
        path = "/commit_cloud/update_checkout_locations"
        data = {
            "repo_name": reponame,
            "workspace": workspace,
            "hostname": hostname,
            "commit": commit,
            "checkout_path": checkoutpath,
            "shared_path": sharedpath,
            "unixname": unixname,
        }
        start = util.timer()
        response = self._send(path, data)
        elapsed = util.timer() - start
        self.ui.debug(
            "response received in %0.2f sec\n" % elapsed, component="commitcloud"
        )

        if "error" in response:
            raise ccerror.ServiceError(self.ui, response["error"])

        self.ui.debug("'update_checkout_locations' successful", component="commitcloud")

    @perftrace.tracefunc("Get Commit Cloud Workspaces")
    def getworkspaces(self, reponame, prefix):
        self.ui.debug("sending 'get_workspaces' request\n", component="commitcloud")

        # send request
        path = "/commit_cloud/get_workspaces"
        data = {"repo_name": reponame, "prefix": prefix}
        start = util.timer()
        response = self._send(path, data)
        elapsed = util.timer() - start
        self.ui.debug(
            "response received in %0.2f sec\n" % elapsed, component="commitcloud"
        )

        if "error" in response:
            raise ccerror.ServiceError(self.ui, response["error"])

        workspaces = response["workspaces_data"]
        return self._makeworkspacesinfo(workspaces)

    @perftrace.tracefunc("Archive/Restore Workspace")
    def updateworkspacearchive(self, reponame, workspace, archived):
        """Archive or Restore the given workspace
        """
        self.ui.debug(
            "sending 'update_workspace_archive' request\n", component="commitcloud"
        )
        path = "/commit_cloud/update_workspace_archive"
        data = {"repo_name": reponame, "workspace": workspace, "archived": archived}
        start = util.timer()
        response = self._send(path, data)
        elapsed = util.timer() - start
        self.ui.debug(
            "response received in %0.2f sec\n" % elapsed, component="commitcloud"
        )
        if "error" in response:
            raise ccerror.ServiceError(self.ui, response["error"])


# Make sure that the HttpsCommitCloudService is a singleton
HttpsCommitCloudService = baseservice.SingletonDecorator(_HttpsCommitCloudService)

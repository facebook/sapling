# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

# Standard Library
import hashlib
import os
import socket
import subprocess

from mercurial import (
    config,
    encoding,
    lock as lockmod,
    obsolete,
    pycompat,
    util,
    vfs as vfsmod,
)
from mercurial.i18n import _

from . import commitcloudcommon
from .. import shareutil


SERVICE = "commitcloud"
ACCOUNT = "commitcloud"


def _gethomevfs(ui, config_option_name):
    """
    Check config first.
    If config override is not given locate home dir
    Unix:
        returns the value of the 'HOME' environment variable
        if it is set and not equal to the empty string
    Windows:
        returns the value of the 'APPDATA' environment variable
        if it is set and not equal to the empty string
    """
    path = ui.config("commitcloud", config_option_name)
    if path and not os.path.isdir(path):
        raise commitcloudcommon.ConfigurationError(
            ui, _("invalid commitcloud.%s '%s'") % (config_option_name, path)
        )
    if path:
        return vfsmod.vfs(util.expandpath(path))

    if pycompat.iswindows:
        envvar = "APPDATA"
    else:
        envvar = "HOME"
    homedir = encoding.environ.get(envvar)
    if not homedir:
        raise commitcloudcommon.ConfigurationError(
            ui, _("$%s environment variable not found") % envvar
        )

    if not os.path.isdir(homedir):
        raise commitcloudcommon.ConfigurationError(
            ui, _("invalid homedir '%s'") % homedir
        )

    return vfsmod.vfs(homedir)


class TokenLocator(object):

    filename = ".commitcloudrc"

    def __init__(self, ui):
        self.ui = ui
        self.vfs = _gethomevfs(self.ui, "user_token_path")
        self.vfs.createmode = 0o600

    def _gettokenfromfile(self):
        """On platforms except macOS tokens are stored in a file"""
        if not self.vfs.exists(self.filename):
            return None
        with self.vfs.open(self.filename, r"rb") as f:
            tokenconfig = config.config()
            tokenconfig.read(self.filename, f)
            return tokenconfig.get("commitcloud", "user_token")

    def _settokentofile(self, token):
        """On platforms except macOS tokens are stored in a file"""
        with self.vfs.open(self.filename, "w") as configfile:
            configfile.write(("[commitcloud]\nuser_token=%s\n") % token)

    def _gettokenosx(self):
        """On macOS tokens are stored in keychain
           this function fetches token from keychain
        """
        p = subprocess.Popen(
            [
                "security",
                "find-generic-password",
                "-g",
                "-s",
                SERVICE,
                "-a",
                ACCOUNT,
                "-w",
            ],
            stdin=None,
            close_fds=util.closefds,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        try:
            text = p.stdout.read()
            if text:
                return text
            else:
                return None
        except Exception as e:
            raise commitcloudcommon.UnexpectedError(self.ui, e)

    def _settokenosx(self, token):
        """On macOS tokens are stored in keychain
           this function puts the token to keychain
        """
        p = subprocess.Popen(
            [
                "security",
                "add-generic-password",
                "-a",
                ACCOUNT,
                "-s",
                SERVICE,
                "-p",
                token,
                "-U",
            ],
            stdin=None,
            close_fds=util.closefds,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )

        try:
            self.ui.debug("new token is stored in keychain\n")
            return p.stdout.read()
        except Exception as e:
            raise commitcloudcommon.UnexpectedError(self.ui, e)

    @property
    def token(self):
        """Public API
            get token
                returns None if token is not found
                it can throw only in case of unexpected error
        """
        if pycompat.isdarwin and not self.ui.config("commitcloud", "user_token_path"):
            token = self._gettokenosx()
        else:
            token = self._gettokenfromfile()
        # Ensure token doesn't have any extraneous whitespace around it.
        if token is not None:
            token = token.strip()
        return token

    def settoken(self, token):
        """Public API
            set token
                it can throw only in case of unexpected error
        """
        # Ensure token doesn't have any extraneous whitespace around it.
        if token is not None:
            token = token.strip()
        if pycompat.isdarwin and not self.ui.config("commitcloud", "user_token_path"):
            self._settokenosx(token)
        else:
            self._settokentofile(token)


class WorkspaceManager(object):
    """
    Set / get current workspace for Commit Cloud
    """

    filename = "commitcloudrc"

    def __init__(self, repo):
        self.ui = repo.ui
        self.repo = repo

    def _getdefaultworkspacename(self):
        """
        Worspace naming convention:
        section/section_name/workspace_name
            where section is one of ('user', 'group', 'team', 'project')
        Examples:
            team/source_control/shared
            user/<username>/default
            project/commit_cloud/default
        """
        return "user/" + util.shortuser(self.ui.username()) + "/default"

    @property
    def reponame(self):
        return self.ui.config(
            "remotefilelog",
            "reponame",
            os.path.basename(self.ui.config("paths", "default")),
        )

    @property
    def workspace(self):
        if self.repo.svfs.exists(self.filename):
            with self.repo.svfs.open(self.filename, r"rb") as f:
                workspaceconfig = config.config()
                workspaceconfig.read(self.filename, f)
                return workspaceconfig.get("commitcloud", "current_workspace")
        else:
            return None

    def setworkspace(self, workspace=None):
        if not workspace:
            workspace = self._getdefaultworkspacename()
        with self.repo.wlock(), self.repo.lock(), self.repo.svfs.open(
            self.filename, "w", atomictemp=True
        ) as f:
            f.write("[commitcloud]\n" "current_workspace=%s\n" % workspace)

    def clearworkspace(self):
        with self.repo.wlock(), self.repo.lock():
            self.repo.svfs.unlink(self.filename)


class SubscriptionManager(object):

    dirname = ".commitcloud"
    joined = "joined"
    default_scm_daemon_port = 15432

    def __init__(self, repo):
        self.ui = repo.ui
        self.repo = repo
        workspacemanager = WorkspaceManager(self.repo)
        self.workspace = workspacemanager.workspace
        self.repo_name = workspacemanager.reponame
        self.repo_root = self.repo.path
        self.vfs = vfsmod.vfs(
            _gethomevfs(self.ui, "connected_subscribers_path").join(
                self.dirname, self.joined
            )
        )
        self.filename_unique = hashlib.sha256(
            "\0".join([self.repo_root, self.repo_name, self.workspace])
        ).hexdigest()[:32]
        self.subscription_enabled = self.ui.configbool(
            "commitcloud", "subscription_enabled", False
        )
        self.scm_daemon_tcp_port = self.ui.configint(
            "commitcloud", "scm_daemon_tcp_port", self.default_scm_daemon_port
        )

    def checksubscription(self):
        if not self.subscription_enabled:
            self.removesubscription()
            return
        if not self.vfs.exists(self.filename_unique):
            with self.vfs.open(self.filename_unique, "w") as configfile:
                configfile.write(
                    ("[commitcloud]\nworkspace=%s\nrepo_name=%s\nrepo_root=%s\n")
                    % (self.workspace, self.repo_name, self.repo_root)
                )
                self._restart_service_subscriptions()
        else:
            self._test_service_is_running()

    def removesubscription(self):
        if self.vfs.exists(self.filename_unique):
            self.vfs.tryunlink(self.filename_unique)
            self._restart_service_subscriptions(warn_service_not_running=False)

    def _warn_service_not_running(self):
        commitcloudcommon.highlightstatus(
            self.ui,
            _(
                "warning: scm daemon is not running and fully automated synchronization may not work\n"
                "please contact %s if this warning persists\n"
            )
            % commitcloudcommon.getownerteam(self.ui),
        )

    def _test_service_is_running(self):
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        if s.connect_ex(("127.0.0.1", self.scm_daemon_tcp_port)):
            self._warn_service_not_running()
        s.close()

    def _restart_service_subscriptions(self, warn_service_not_running=True):
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            s.connect(("127.0.0.1", self.scm_daemon_tcp_port))
            s.send('["restart"]')
        except socket.error:
            if warn_service_not_running:
                self._warn_service_not_running()
        finally:
            s.close()


# Obsmarker syncing.
#
# To avoid lock interactions between transactions (which may add new obsmarkers)
# and sync (which wants to clear the obsmarkers), we use a two-stage process to
# sync obsmarkers.
#
# When a transaction completes, we append any new obsmarkers to the pending
# obsmarker file.  This is protected by the obsmarkers lock.
#
# When a sync operation starts, we transfer these obsmarkers to the syncing
# obsmarker file.  This transfer is also protected by the obsmarkers lock, but
# the transfer should be very quick as it's just moving a small amount of data.
#
# The sync process can upload the syncing obsmarkers at its leisure.  The
# syncing obsmarkers file is protected by the infinitepush backup lock.

_obsmarkerslockname = "commitcloudpendingobsmarkers.lock"
_obsmarkerslocktimeout = 2
_obsmarkerspending = "commitcloudpendingobsmarkers"
_obsmarkerssyncing = "commitcloudsyncingobsmarkers"


def addpendingobsmarkers(repo, markers):
    with lockmod.lock(repo.svfs, _obsmarkerslockname, timeout=_obsmarkerslocktimeout):
        with repo.svfs.open(_obsmarkerspending, "ab") as f:
            offset = f.tell()
            # offset == 0: new file - add the version header
            data = b"".join(
                obsolete.encodemarkers(markers, offset == 0, obsolete._fm1version)
            )
            f.write(data)


def getsyncingobsmarkers(repo):
    """Transfers any pending obsmarkers, and returns all syncing obsmarkers.

    The caller must hold the backup lock.
    """
    # Move any new obsmarkers from the pending file to the syncing file
    srcrepo = shareutil.getsrcrepo(repo)
    with lockmod.lock(repo.svfs, _obsmarkerslockname, timeout=_obsmarkerslocktimeout):
        if repo.svfs.exists(_obsmarkerspending):
            with repo.svfs.open(_obsmarkerspending) as f:
                _version, markers = obsolete._readmarkers(f.read())
            with srcrepo.vfs.open(_obsmarkerssyncing, "ab") as f:
                offset = f.tell()
                # offset == 0: new file - add the version header
                data = b"".join(
                    obsolete.encodemarkers(markers, offset == 0, obsolete._fm1version)
                )
                f.write(data)
            repo.svfs.unlink(_obsmarkerspending)

    # Load the syncing obsmarkers
    markers = []
    if srcrepo.vfs.exists(_obsmarkerssyncing):
        with srcrepo.vfs.open(_obsmarkerssyncing) as f:
            _version, markers = obsolete._readmarkers(f.read())
    return markers


def clearsyncingobsmarkers(repo):
    """Clears all syncing obsmarkers.  The caller must hold the backup lock."""
    srcrepo = shareutil.getsrcrepo(repo)
    srcrepo.vfs.unlink(_obsmarkerssyncing)


def getworkspacename(repo):
    return WorkspaceManager(repo).workspace

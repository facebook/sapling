# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import hashlib
import os
import socket

from edenscm.mercurial import vfs as vfsmod
from edenscm.mercurial.i18n import _

from . import error as ccerror, util as ccutil, workspace


class SubscriptionManager(object):
    def __init__(self, repo):
        self.ui = repo.ui
        self.repo = repo
        self.workspacename = workspace.currentworkspace(repo)
        self.repo_name = ccutil.getreponame(repo)
        self.repo_root = self.repo.path
        self.vfs = vfsmod.vfs(
            os.path.join(
                ccutil.getuserconfigpath(self.ui, "connected_subscribers_path"),
                ".commitcloud",
                "joined",
            )
        )
        self.filename_unique = hashlib.sha256(
            "\0".join([self.repo_root, self.repo_name, self.workspacename])
        ).hexdigest()[:32]
        self.subscription_enabled = self.ui.configbool(
            "commitcloud", "subscription_enabled"
        )
        self.scm_daemon_tcp_port = self.ui.configint(
            "commitcloud", "scm_daemon_tcp_port"
        )

    def checksubscription(self):
        if not self.subscription_enabled:
            self.removesubscription()
            return
        if not self.vfs.exists(self.filename_unique):
            with self.vfs.open(self.filename_unique, "w") as configfile:
                configfile.write(
                    ("[commitcloud]\nworkspace=%s\nrepo_name=%s\nrepo_root=%s\n")
                    % (self.workspacename, self.repo_name, self.repo_root)
                )
                self._restart_service_subscriptions()
        else:
            self._test_service_is_running()

    def removesubscription(self):
        if self.vfs.exists(self.filename_unique):
            self.vfs.tryunlink(self.filename_unique)
            self._restart_service_subscriptions(warn_service_not_running=False)

    def _warn_service_not_running(self):
        self.ui.status(
            _(
                "scm daemon is not running and automatic synchronization may not work\n"
                "(run 'hg cloud sync' manually if your workspace is not synchronized)\n"
                "(please contact %s if this warning persists)\n"
            )
            % ccerror.getownerteam(self.ui),
            component="commitcloud",
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
            s.send('["commitcloud::restart_subscriptions", {}]')
        except socket.error:
            if warn_service_not_running:
                self._warn_service_not_running()
        finally:
            s.close()

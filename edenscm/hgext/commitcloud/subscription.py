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


def _uniquefilename(reporoot, reponame, workspacename):
    hash = hashlib.sha256("\0".join([reporoot, reponame, workspacename])).hexdigest()
    return hash[:32]


def _subscriptionvfs(repo):
    return vfsmod.vfs(
        os.path.join(
            ccutil.getuserconfigpath(repo.ui, "connected_subscribers_path"),
            ".commitcloud",
            "joined",
        )
    )


def check(repo):
    if not repo.ui.configbool("commitcloud", "subscription_enabled"):
        remove(repo)
        return
    reponame = ccutil.getreponame(repo)
    workspacename = workspace.currentworkspace(repo)
    if not workspacename:
        return
    filename = _uniquefilename(repo.path, reponame, workspacename)
    vfs = _subscriptionvfs(repo)
    if not vfs.exists(filename):
        with vfs.open(filename, "w") as configfile:
            configfile.write(
                ("[commitcloud]\nworkspace=%s\nrepo_name=%s\nrepo_root=%s\n")
                % (workspacename, reponame, repo.path)
            )
            _restart_service_subscriptions(repo.ui)
    else:
        _test_service_is_running(repo.ui)


def remove(repo):
    reponame = ccutil.getreponame(repo)
    workspacename = workspace.currentworkspace(repo)
    if not workspacename:
        return
    filename = _uniquefilename(repo.path, reponame, workspacename)
    vfs = _subscriptionvfs(repo)
    if vfs.exists(filename):
        vfs.tryunlink(filename)
        _restart_service_subscriptions(repo.ui, warn_service_not_running=False)


def _warn_service_not_running(ui):
    ui.status(
        _(
            "scm daemon is not running and automatic synchronization may not work\n"
            "(run 'hg cloud sync' manually if your workspace is not synchronized)\n"
            "(please contact %s if this warning persists)\n"
        )
        % ccerror.getownerteam(ui),
        component="commitcloud",
    )


def _test_service_is_running(ui):
    port = ui.configint("commitcloud", "scm_daemon_tcp_port")
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    if s.connect_ex(("127.0.0.1", port)):
        _warn_service_not_running()
    s.close()


def _restart_service_subscriptions(ui, warn_service_not_running=True):
    port = ui.configint("commitcloud", "scm_daemon_tcp_port")
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        s.connect(("127.0.0.1", port))
        s.send('["commitcloud::restart_subscriptions", {}]')
    except socket.error:
        if warn_service_not_running:
            _warn_service_not_running(ui)
    finally:
        s.close()

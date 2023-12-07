# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import hashlib
import os
import socket

from sapling import vfs as vfsmod
from sapling.i18n import _
from sapling.pycompat import encodeutf8

from . import error as ccerror, util as ccutil, workspace


def _uniquefilename(reporoot, reponame, workspacename):
    # Stabilize filename for tests.
    if testtmp := os.getenv("TESTTMP"):
        if reporoot.startswith(testtmp):
            reporoot = "$TESTTMP/" + reporoot.removeprefix(testtmp).replace("\\", "/")

    hash = hashlib.sha256(
        encodeutf8("\0".join([reporoot, reponame, workspacename]))
    ).hexdigest()
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

    # We previously tracked subscriptions using the "local" repo.path, but that
    # results in duplicate subscriptions recorded for shared repos. Now we use
    # the shared path, but also clean up old entries, if present.
    filename = _uniquefilename(repo.sharedpath, reponame, workspacename)
    oldfilename = _uniquefilename(repo.path, reponame, workspacename)

    vfs = _subscriptionvfs(repo)
    didsomething = False
    if not vfs.exists(filename):
        with vfs.open(filename, "wb") as configfile:
            repo.ui.debug(
                "check: writing subscription %s\n" % filename, component="commitcloud"
            )
            configfile.write(
                encodeutf8(
                    "[commitcloud]\nworkspace=%s\nrepo_name=%s\nrepo_root=%s\n"
                    % (workspacename, reponame, repo.sharedpath)
                )
            )
            didsomething = True

    if filename != oldfilename and vfs.exists(oldfilename):
        repo.ui.debug(
            "check: cleaning up non-shared subscription %s\n" % oldfilename,
            component="commitcloud",
        )
        vfs.tryunlink(oldfilename)

    if didsomething:
        _restart_subscriptions(repo.ui)
    else:
        _test_service_is_running(repo.ui)


def remove(repo):
    reponame = ccutil.getreponame(repo)
    workspacename = workspace.currentworkspace(repo)
    if not workspacename:
        return

    # We previously tracked subscriptions using the "local" repo.path, but that
    # results in duplicate subscriptions recorded for shared repos. Now we use
    # the shared path, but also clean up old entries, if present.
    filename = _uniquefilename(repo.sharedpath, reponame, workspacename)
    oldfilename = _uniquefilename(repo.path, reponame, workspacename)

    vfs = _subscriptionvfs(repo)

    didsomething = False
    if vfs.exists(filename):
        vfs.tryunlink(filename)
        didsomething = True
        repo.ui.debug(
            "remove: cleaning up shared subscription %s\n" % filename,
            component="commitcloud",
        )
    if vfs.exists(oldfilename):
        vfs.tryunlink(oldfilename)
        didsomething = True
        repo.ui.debug(
            "remove: cleaning up non-shared subscription %s\n" % oldfilename,
            component="commitcloud",
        )

    if didsomething:
        _restart_subscriptions(repo.ui, warn_service_not_running=False)


def move(repo, workspace, new_workspace):
    reponame = ccutil.getreponame(repo)
    if not workspace or not new_workspace:
        return

    # We previously tracked subscriptions using the "local" repo.path, but that
    # results in duplicate subscriptions recorded for shared repos. Now we use
    # the shared path, but also clean up old entries, if present.

    src = _uniquefilename(repo.sharedpath, reponame, workspace)
    oldsrc = _uniquefilename(repo.path, reponame, workspace)

    dst = _uniquefilename(repo.sharedpath, reponame, new_workspace)
    olddst = _uniquefilename(repo.path, reponame, new_workspace)

    vfs = _subscriptionvfs(repo)

    if vfs.exists(src) or vfs.exists(oldsrc):
        vfs.tryunlink(src)
        vfs.tryunlink(oldsrc)
        with vfs.open(dst, "wb") as configfile:
            configfile.write(
                encodeutf8(
                    "[commitcloud]\nworkspace=%s\nrepo_name=%s\nrepo_root=%s\n"
                    % (new_workspace, reponame, repo.sharedpath)
                )
            )
        if olddst != dst and vfs.exists(olddst):
            repo.ui.debug(
                "move: cleaning up non-shared subscription %s\n" % olddst,
                component="commitcloud",
            )
            vfs.tryunlink(olddst)
        _restart_subscriptions(repo.ui, warn_service_not_running=False)


def _warn_service_not_running(ui):
    ui.status(
        _(
            "scm daemon is not running and automatic synchronization may not work\n"
            "(run '@prog@ cloud sync' manually if your workspace is not synchronized)\n"
            "(please contact %s if this warning persists)\n"
        )
        % ccerror.getsupportcontact(ui),
        component="commitcloud",
    )


def _test_service_is_running(ui):
    port = ui.configint("commitcloud", "scm_daemon_tcp_port")
    if port < 0:
        return
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    if s.connect_ex(("127.0.0.1", port)):
        _warn_service_not_running(ui)
    s.close()


def testservicestatus(repo):
    if not repo.ui.configbool("commitcloud", "subscription_enabled"):
        return False
    port = repo.ui.configint("commitcloud", "scm_daemon_tcp_port")
    if port < 0:
        return
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    status = s.connect_ex(("127.0.0.1", port)) == 0
    s.close()
    return status


def _restart_subscriptions(ui, warn_service_not_running=True):
    port = ui.configint("commitcloud", "scm_daemon_tcp_port")
    if port < 0:
        return
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        s.connect(("127.0.0.1", port))
        s.send(b'["commitcloud::restart_subscriptions", {}]')
    except socket.error:
        if warn_service_not_running:
            _warn_service_not_running(ui)
    finally:
        s.close()

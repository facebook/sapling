# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import socket

from edenscm import perftrace, util

from . import service, util as ccutil, workspace


@perftrace.tracefunc("Checkout Locations Sync")
def send(ui, repo, parent1, **kwargs):
    try:
        ui.debug("sending new checkout location to commit cloud: %s\n" % parent1)
        start = util.timer()
        commit = parent1
        reponame = ccutil.getreponame(repo)
        workspacename = workspace.currentworkspace(repo)
        if workspacename is None:
            workspacename = workspace.defaultworkspace(ui)
        if workspacename is None:
            return
        serv = service.get(ui)
        hostname = socket.gethostname()
        sharedpath = repo.sharedpath
        checkoutpath = repo.path
        unixname = ui.username()
        serv.updatecheckoutlocations(
            reponame,
            workspacename,
            hostname,
            commit,
            checkoutpath,
            sharedpath,
            unixname,
        )

        elapsed = util.timer() - start
        ui.debug("finished in %0.2f sec\n" % elapsed)
    except Exception as e:
        ui.debug("syncing checkout locations failed with error: %s" % str(e))

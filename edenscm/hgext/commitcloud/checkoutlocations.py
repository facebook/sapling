# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import socket

from edenscm.mercurial import perftrace, util

from . import service, token, util as ccutil, workspace


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
        tokenlocator = token.TokenLocator(ui)
        serv = service.get(ui, tokenlocator.token)
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
        pass

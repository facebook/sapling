# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


import socket

from sapling import error, git

from . import gitservice, localservice, saplingremoteapiservice


def get(ui, repo=None):
    servicetype = ui.config("commitcloud", "servicetype")
    if servicetype == "local":
        return localservice.LocalService(ui)
    elif git.isgitformat(repo):
        return gitservice.GitService(ui, repo)
    elif (
        servicetype == "remote"
        or servicetype == "saplingremoteapi"
        or servicetype == "edenapi"
    ):
        return saplingremoteapiservice.SaplingRemoteAPIService(
            ui,
            repo,
        )
    else:
        msg = "Unrecognized commitcloud.servicetype: %s" % servicetype
        raise error.Abort(msg)


def makeclientinfo(repo, syncstate):
    hostname = repo.ui.config("commitcloud", "hostname", socket.gethostname())
    return {
        "hostname": hostname,
        "reporoot": repo.root,
        "version": syncstate.version,
    }

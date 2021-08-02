# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial.edenapi_upload import getreponame


def createremote(ui, repo, **opts):
    # TODO(yancouto): Pipe command through and implement logic
    status = repo.status()

    # Until we get a functional snapshot end to end, let's only consider modifed
    # files. Later, we'll add all other types of files.
    _stream, _stats = repo.edenapi.uploadsnapshot(
        getreponame(repo), {"files": {"modified": status.modified}}
    )

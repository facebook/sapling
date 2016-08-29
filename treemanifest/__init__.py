# __init__.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import (
    cmdutil,
    localrepo,
)
from remotefilelog.contentstore import unioncontentstore
from remotefilelog.datapack import datapackstore
from remotefilelog import shallowutil
import ctreemanifest

cmdtable = {}
command = cmdutil.command(cmdtable)

PACK_CATEGORY='manifest'

def reposetup(ui, repo):
    wraprepo(repo)

def wraprepo(repo):
    if not isinstance(repo, localrepo.localrepository):
        return

    packpath = shallowutil.getpackpath(repo, PACK_CATEGORY)
    datastore = datapackstore(
        packpath,
        usecdatapack=repo.ui.configbool('remotefilelog', 'fastdatapack'))
    repo.svfs.manifestdatastore = unioncontentstore(datastore)

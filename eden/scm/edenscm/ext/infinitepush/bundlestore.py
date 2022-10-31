# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Infinitepush Bundle Store
"""store for infinitepush bundles"""

import hashlib
import os
import subprocess
from tempfile import NamedTemporaryFile

from edenscm import error
from edenscm.i18n import _


class bundlestore(object):
    def __init__(self, repo):
        self.store = filebundlestore(repo)
        from . import fileindex

        self.index = fileindex.fileindex(repo)


class filebundlestore(object):
    """bundle store in filesystem

    meant for storing bundles somewhere on disk and on network filesystems
    """

    def __init__(self, repo):
        self.storepath = repo.ui.configpath("scratchbranch", "storepath")
        if not self.storepath:
            self.storepath = repo.localvfs.join("scratchbranches", "filebundlestore")
        if not os.path.exists(self.storepath):
            os.makedirs(self.storepath)

    def _dirpath(self, hashvalue):
        """First two bytes of the hash are the name of the upper
        level directory, next two bytes are the name of the
        next level directory"""
        return os.path.join(self.storepath, hashvalue[0:2], hashvalue[2:4])

    def _filepath(self, filename):
        return os.path.join(self._dirpath(filename), filename)

    def write(self, data):
        filename = hashlib.sha1(data).hexdigest()
        dirpath = self._dirpath(filename)

        if not os.path.exists(dirpath):
            os.makedirs(dirpath)

        with open(self._filepath(filename), "wb") as f:
            f.write(data)

        return filename

    def read(self, key):
        try:
            f = open(self._filepath(key), "rb")
        except IOError:
            return None

        return f.read()

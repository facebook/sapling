# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

# based on bundleheads extension by Gregory Szorc <gps@mozilla.com>

import abc
import hashlib
import os
from mercurial import util

class BundleWriteException(Exception):
    pass

class BundleReadException(Exception):
    pass

class abstractbundlestore(object):
    """Defines the interface for bundle stores.

    A bundle store is an entity that stores raw bundle data. It is a simple
    key-value store. However, the keys are chosen by the store. The keys can
    be any Python object understood by the corresponding bundle index (see
    ``abstractbundleindex`` below).
    """
    __metaclass__ = abc.ABCMeta

    @abc.abstractmethod
    def write(self, data):
        """Write bundle data to the store.

        This function receives the raw data to be written as a str.
        Throws BundleWriteException
        The key of the written data MUST be returned.
        """

    @abc.abstractmethod
    def read(self, key):
        """Obtain bundle data for a key.

        Returns None if the bundle isn't known.
        Throws BundleReadException
        The returned object should be a file object supporting read()
        and close().
        """

class filebundlestore(object):
    """bundle store in filesystem

    meant for storing bundles somewhere on disk and on network filesystems
    """
    def __init__(self, ui, repo):
        self.ui = ui
        self.repo = repo
        self.storepath = ui.configpath('scratchbranch', 'storepath')
        if not self.storepath:
            self.storepath = self.repo.vfs.join("scratchbranches",
                                                "filebundlestore")
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

        with open(self._filepath(filename), 'w') as f:
            f.write(data)

        return filename

    def read(self, key):
        try:
            f = open(self._filepath(key), 'r')
        except IOError:
            return None

        return f.read()

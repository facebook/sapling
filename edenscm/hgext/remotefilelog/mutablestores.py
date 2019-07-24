# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from edenscm.mercurial.node import hex
from edenscmnative.bindings import revisionstore

from . import shallowutil
from .historypack import mutablehistorypack


class mutablebasestore(object):
    def __init__(self):
        self._store = None

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        if exc_type is None:
            self._store.flush()
        self._store = None

    def __getattr__(self, name):
        return getattr(self._store, name)


class mutabledatastore(mutablebasestore):
    @staticmethod
    def makestore(repo, path):
        shallowutil.mkstickygroupdir(repo.ui, path)
        return revisionstore.mutabledeltastore(packfilepath=path)

    def __init__(self, repo, path):
        super(mutabledatastore, self).__init__()

        self._store = self.makestore(repo, path)


class mutablehistorystore(mutablebasestore):
    @staticmethod
    def makestore(repo, path):
        if repo.ui.configbool("format", "userustmutablestore", True):
            shallowutil.mkstickygroupdir(repo.ui, path)
            return revisionstore.mutablehistorystore(packfilepath=path)
        else:
            return mutablehistorypack(repo.ui, path)

    def __init__(self, repo, path):
        super(mutablehistorystore, self).__init__()
        self._store = self.makestore(repo, path)


class pendingmutablepack(object):
    def __init__(self, repo, pathcb):
        self._mutabledpack = None
        self._mutablehpack = None

        self._repo = repo
        self._pathcb = pathcb

    def getmutabledpack(self, read=False):
        if self._mutabledpack is None and not read:
            path = self._pathcb()
            self._mutabledpack = mutabledatastore.makestore(self._repo, path)

        return self._mutabledpack

    def getmutablehpack(self, read=False):
        if self._mutablehpack is None and not read:
            path = self._pathcb()
            self._mutablehpack = mutablehistorystore.makestore(self._repo, path)

        return self._mutablehpack

    def getmutablepack(self):
        dpack = self.getmutabledpack()
        hpack = self.getmutablehpack()

        return dpack, hpack

    def commit(self):
        dpackpath = None
        hpackpath = None

        if self._mutabledpack is not None:
            try:
                dpackpath = self._mutabledpack.flush()
            finally:
                self._mutabledpack = None

        if self._mutablehpack is not None:
            try:
                hpackpath = self._mutablehpack.flush()
            finally:
                self._mutablehpack = None

        return dpackpath, hpackpath

    def abort(self):
        self._mutabledpack = None
        self._mutablehpack = None


class mutabledatahistorystore(object):
    """A proxy class that gets added to the union store and knows how to answer
    requests by inspecting the current mutable data and history packs. We can't
    insert the mutable packs themselves into the union store because they can be
    created and destroyed over time."""

    def __init__(self, getpendingpacks):
        self.getpendingpacks = getpendingpacks

    def getmissing(self, keys):
        dpack = self.getpendingpacks().getmutabledpack(True)
        if dpack is None:
            return keys

        return dpack.getmissing(keys)

    def get(self, name, node):
        dpack = self.getpendingpacks().getmutabledpack(True)
        if dpack is None:
            raise KeyError(name, hex(node))

        return dpack.get(name, node)

    def getdelta(self, name, node):
        dpack = self.getpendingpacks().getmutabledpack(True)
        if dpack is None:
            raise KeyError(name, hex(node))

        return dpack.getdelta(name, node)

    def getdeltachain(self, name, node):
        dpack = self.getpendingpacks().getmutabledpack(True)
        if dpack is None:
            raise KeyError(name, hex(node))

        return dpack.getdeltachain(name, node)

    def getmeta(self, name, node):
        dpack = self.getpendingpacks().getmutabledpack(True)
        if dpack is None:
            raise KeyError(name, hex(node))

        return dpack.getmeta(name, node)

    def getnodeinfo(self, name, node):
        hpack = self.getpendingpacks().getmutablehpack(True)
        if hpack is None:
            raise KeyError(name, hex(node))

        return hpack.getnodeinfo(name, node)

    def getancestors(self, name, node, known=None):
        hpack = self.getpendingpacks().getmutablehpack(True)
        if hpack is None:
            raise KeyError(name, hex(node))

        return hpack.getancestors(name, node, known=known)

    def getmetrics(self):
        return {}

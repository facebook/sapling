# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# connectionpool.py - class for pooling peer connections for reuse

from __future__ import absolute_import

import time

from . import extensions, sshpeer, util


class connectionpool(object):
    def __init__(self, repo):
        self._repo = repo
        self._pool = dict()

    def get(self, path, opts=None):
        # Prevent circular dependency
        from . import hg

        if opts is None:
            opts = {}
        if opts.get("ssh") or opts.get("remotecmd"):
            self._repo.ui.debug(
                "not using connection pool due to ssh or "
                "remotecmd option being set\n"
            )
            peer = hg.peer(self._repo.ui, opts, path)
            return standaloneconnection(peer)

        pathpool = self._pool.get(path)
        if pathpool is None:
            pathpool = list()
            self._pool[path] = pathpool

        conn = None
        if len(pathpool) > 0:
            try:
                conn = pathpool.pop()
                peer = conn.peer
                # If the connection has died, drop it
                if isinstance(peer, sshpeer.sshpeer) and util.safehasattr(
                    peer, "_subprocess"
                ):
                    proc = peer._subprocess
                    if proc.poll() is not None:
                        conn = None
                # If the connection has expired, close it
                if conn is not None and conn.expired():
                    self._repo.ui.debug(
                        "not reusing expired connection to %s\n" % conn.path
                    )
                    conn.close()
                    conn = None
            except IndexError:
                pass

        if conn is None:

            def _cleanup(orig):
                # close pipee first so peer._cleanup reading it won't deadlock,
                # if there are other processes with pipeo open (i.e. us).
                peer = orig.im_self
                if util.safehasattr(peer, "_pipee"):
                    peer._pipee.close()
                return orig()

            peer = hg.peer(self._repo.ui, {}, path)
            if util.safehasattr(peer, "_cleanup"):
                extensions.wrapfunction(peer, "_cleanup", _cleanup)

            conn = connection(self._repo.ui, pathpool, peer, path)

        return conn

    def close(self):
        for pathpool in self._pool.itervalues():
            for conn in pathpool:
                conn.close()
            del pathpool[:]


class standaloneconnection(object):
    def __init__(self, peer):
        self.peer = peer

    def __enter__(self):
        return self

    def __exit__(self, type, value, traceback):
        self.close()

    def close(self):
        if util.safehasattr(self.peer, "_cleanup"):
            self.peer._cleanup()


class connection(object):
    def __init__(self, ui, pool, peer, path):
        self._ui = ui
        self._pool = pool
        self.peer = peer
        self.path = path
        self.expiry = None
        lifetime = ui.configint("connectionpool", "lifetime")
        if lifetime is not None:
            self.expiry = time.time() + lifetime

    def __enter__(self):
        return self

    def __exit__(self, type, value, traceback):
        # Only add the connection back to the pool if there was no exception,
        # since an exception could mean the connection is not in a reusable
        # state.
        if type is not None:
            self.close()
        elif self.expired():
            self._ui.debug("closing expired connection to %s\n" % self.path)
            self.close()
        else:
            self._pool.append(self)

    def expired(self):
        return self.expiry is not None and time.time() > self.expiry

    def close(self):
        if util.safehasattr(self.peer, "_cleanup"):
            self.peer._cleanup()

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# connectionpool.py - class for pooling peer connections for reuse

from __future__ import absolute_import

import os
import time

from . import json, pycompat, sshpeer, util


class connectionpool:
    def __init__(self, repo):
        self._real_connection_pool = realconnectionpool(repo)

    def get(self, path, opts=None, reason="default"):
        return lazyconnection(self._real_connection_pool, path, opts, reason)

    def close(self):
        self._real_connection_pool.close()


# Internal implementation would connect lazily.


class lazyconnection:
    def __init__(self, connection_pool, path, opts, reason):
        self.peer = lazypeer(connection_pool, path, opts, reason)

    def __enter__(self):
        return self

    def __exit__(self, type, value, traceback):
        if self.peer._real_connection:
            self.peer._real_connection.__exit__(type, value, traceback)

    def close(self):
        if self.peer._real_connection:
            self.peer._real_connection.close()


class lazypeer:
    def __init__(self, connection_pool, path, opts, reason):
        self._real_connection_pool = connection_pool
        self._path = path
        self._opts = opts
        self._reason = reason
        self._url = path
        self._real_peer = None
        self._real_connection = None

    def url(self):
        if self._real_peer:
            return self._real_peer.url()
        return self._url

    def __getattr__(self, name):
        if not self._real_peer:
            self._real_connection = self._real_connection_pool.get(
                self._path, self._opts, self._reason
            )
            self._real_peer = self._real_connection.peer
        return getattr(self._real_peer, name)


class realconnectionpool:
    def __init__(self, repo):
        self._repo = repo
        self._poolpid = os.getpid()
        self._pool = dict()
        self._reasons = dict()

    def get(self, path, opts=None, reason="default"):
        # Prevent circular dependency
        from . import hg

        # If the process forks we need to use new connections.
        pid = os.getpid()
        if pid != self._poolpid:
            self.close()
            self._poolpid = pid

        if opts is None:
            opts = {}

        if opts.get("ssh") or opts.get("remotecmd"):
            self._repo.ui.debug(
                "not using connection pool due to ssh or "
                "remotecmd option being set\n"
            )
            peer = hg.peer(self._repo.ui, opts, path)
            self.recordreason(reason, path, peer)
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
                if isinstance(peer, sshpeer.sshpeer) and hasattr(peer, "_subprocess"):
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
            peer = hg.peer(self._repo.ui, {}, path)
            conn = realconnection(self._repo.ui, pathpool, peer, path)
        else:
            self._repo.ui.debug("reusing connection from pool\n")

        self.recordreason(reason, path, conn.peer)
        return conn

    def recordreason(self, reason, path, peer):
        peersforreason = self._reasons.setdefault(reason, [])

        realhostname = getattr(peer, "_realhostname", None)
        peerinfo = getattr(peer, "_peerinfo", None)
        peersforreason.append((path, realhostname, peerinfo))

    def reportreasons(self):
        ui = self._repo.ui
        nondefaultreasons = {}
        serverpaths = {}
        for reason, peersforreason in self._reasons.items():
            for (path, peername, peerinfo) in peersforreason:
                serverpaths[reason] = path

                if peername is None:
                    continue

                if reason == "default":
                    # default reason logged directly, to support
                    # any existent queries
                    ui.log(
                        "connectionpool",
                        server_realhostname=peername,
                        server_session=peerinfo.get("session"),
                    )
                    break

                nondefaultreasons[reason] = {"name": peername, "info": peerinfo}
                # we only ever report a single peer for a given reason
                # for the ease of querying.
                break

        nondefaultreasons = json.dumps(nondefaultreasons)
        serverpaths = json.dumps(serverpaths)

        ui.log(
            "connectionpool",
            server_realhostnames_other=nondefaultreasons,
            server_paths=serverpaths,
        )

    def close(self):
        self.reportreasons()
        for pathpool in pycompat.itervalues(self._pool):
            for conn in pathpool:
                conn.close()
            del pathpool[:]


class standaloneconnection:
    def __init__(self, peer):
        self.peer = peer

    def __enter__(self):
        return self

    def __exit__(self, type, value, traceback):
        self.close()

    def close(self):
        if hasattr(self.peer, "_cleanup"):
            self.peer._cleanup()


class realconnection:
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
        if hasattr(self.peer, "_cleanup"):
            self.peer._cleanup()

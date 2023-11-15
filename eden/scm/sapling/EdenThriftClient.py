# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Mercurial extension for supporting eden client checkouts.

This overrides the dirstate to check with the eden daemon for modifications,
instead of doing a normal scan of the filesystem.
"""

import bindings

from . import util

from .node import nullid


class EdenThriftClient:
    def __init__(self, repo):
        self._repo = repo
        self._ui = repo.ui
        # EdenFsClient will recreate the unix domain socket connection per
        # API call. No need to recreate the client every time.
        self._client = repo._rsrepo.workingcopy().edenclient()

    def setHgParents(self, p1, p2):
        if p2 == nullid:
            p2 = None

        p1tree = self._repo[p1].manifestnode()
        self._client.set_parents(p1, p2, p1tree)

    @util.timefunction("edenclientstatus", 0, "_ui")
    def getStatus(self, parent, list_ignored):  # noqa: C901

        # If we are in a pending transaction the parent commit we are querying against
        # might not have been stored to disk yet.  Flush the pending transaction state
        # before asking Eden about the status.
        self._flushPendingTransactions()

        return self._client.get_status(parent, list_ignored)

    @util.timefunction("edenclientcheckout", 0, "_ui")
    def checkout(self, node, checkout_mode, need_flush=True, manifest=None):
        if need_flush:
            self._flushPendingTransactions()

        if manifest is None:
            manifest = self._repo[node].manifestnode()

        return self._client.checkout(node, manifest, checkout_mode)

    def _flushPendingTransactions(self):
        # If a transaction is currently in progress, make sure it has flushed
        # pending commit data to disk so that eden will be able to access it.
        txn = self._repo.currenttransaction()
        if txn is not None:
            txn.writepending()

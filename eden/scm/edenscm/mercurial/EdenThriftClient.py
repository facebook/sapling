# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Mercurial extension for supporting eden client checkouts.

This overrides the dirstate to check with the eden daemon for modifications,
instead of doing a normal scan of the filesystem.
"""

import os
import sys

import toml
from thrift.Thrift import TApplicationException

from . import demandimport, error, node, pycompat, util
from .i18n import _


if sys.version_info < (2, 7, 6):
    # 2.7.6 was the first version to allow unicode format strings in
    # struct.{pack,unpack}; our devservers have 2.7.5, so let's
    # monkey patch in support for unicode format strings.
    import functools
    import struct

    # We disable F821 below because we know we are in Python 2.x based on the
    # sys.version_info check above.

    def pack(orig, fmt, *args):
        if isinstance(fmt, pycompat.unicode):  # noqa: F821
            fmt = fmt.encode("utf-8")
        return orig(fmt, *args)

    def unpack(orig, fmt, data):
        if isinstance(fmt, pycompat.unicode):  # noqa: F821
            fmt = fmt.encode("utf-8")
        return orig(fmt, data)

    struct.pack = functools.partial(pack, struct.pack)
    struct.unpack = functools.partial(unpack, struct.unpack)

# Disable demandimport while importing thrift files.
#
# The thrift modules try importing modules which may or may not exist, and they
# handle the ImportError generated if the modules aren't present.  demandimport
# breaks this behavior by making it appear like the modules were successfully
# loaded, and only throwing ImportError later when you actually try to use
# them.
with demandimport.deactivated():
    import eden.thrift.legacy as eden_thrift_module
    import facebook.eden.ttypes as eden_ttypes

create_thrift_client = eden_thrift_module.create_thrift_client
ScmFileStatus = eden_ttypes.ScmFileStatus
GetScmStatusParams = eden_ttypes.GetScmStatusParams
CheckoutMode = eden_ttypes.CheckoutMode
ConflictType = eden_ttypes.ConflictType
FileInformationOrError = eden_ttypes.FileInformationOrError
NoValueForKeyError = eden_ttypes.NoValueForKeyError
EdenError = eden_ttypes.EdenError


class EdenThriftClient(object):
    def __init__(self, repo):
        self._repo = repo
        self._root = repo.root
        self._ui = repo.ui
        if pycompat.iswindows:
            tomlconfig = toml.load(os.path.join(self._root, ".eden", "config"))
            self._eden_root = tomlconfig["Config"]["root"]
            self._socket_path = tomlconfig["Config"]["socket"]
        else:
            self._socket_path = os.readlink(os.path.join(self._root, ".eden", "socket"))
            # Read the .eden/root symlink to see what eden thinks the name of this
            # mount point is.  This might not match self._root in some cases.  In
            # particular, a parent directory of the eden mount might be bind
            # mounted somewhere else, resulting in it appearing at multiple
            # separate locations.
            self._eden_root = os.readlink(os.path.join(self._root, ".eden", "root"))

    def _get_client(self):
        """
        Create a new client instance for each call because we may be idle
        (from the perspective of the server) between calls and have our
        connection snipped by the server.
        We could potentially try to speculatively execute a call and
        reconnect on transport failure, but for the moment this strategy
        is a reasonable compromise.
        """
        return create_thrift_client(socket_path=self._socket_path)

    def setHgParents(self, p1, p2, need_flush=True, p1manifest=None):
        if p2 == node.nullid:
            p2 = None

        if need_flush:
            self._flushPendingTransactions()

        parents = eden_ttypes.WorkingDirectoryParents(parent1=p1, parent2=p2)
        params = eden_ttypes.ResetParentCommitsParams(hgRootManifest=p1manifest)
        with self._get_client() as client:
            client.resetParentCommits(self._eden_root, parents, params)

    @util.timefunction("edenclientstatus", 0, "_ui")
    def getStatus(self, parent, list_ignored):  # noqa: C901

        # If we are in a pending transaction the parent commit we are querying against
        # might not have been stored to disk yet.  Flush the pending transaction state
        # before asking Eden about the status.
        self._flushPendingTransactions()

        with self._get_client() as client:
            try:
                edenstatus = client.getScmStatusV2(
                    GetScmStatusParams(self._eden_root, parent, list_ignored)
                ).status.entries
            except TApplicationException as e:
                # Fallback to old getScmStatus in the case that this is running
                # against an older version of edenfs in which getScmStatusV2 is
                # not known
                if e.type == TApplicationException.UNKNOWN_METHOD:
                    edenstatus = client.getScmStatus(
                        self._eden_root, list_ignored, parent
                    ).entries
                else:
                    raise
            except EdenError as e:
                raise error.Abort(_("cannot fetch eden status: %s") % e.message)

            return edenstatus

    @util.timefunction("edenclientcheckout", 0, "_ui")
    def checkout(self, node, checkout_mode, need_flush=True, manifest=None):
        if need_flush:
            self._flushPendingTransactions()
        params = eden_ttypes.CheckOutRevisionParams(hgRootManifest=manifest)
        with self._get_client() as client:
            try:
                return client.checkOutRevision(
                    self._eden_root, node, checkout_mode, params
                )
            except EdenError as e:
                raise error.Abort(_("error performing EdenFS checkout: %s") % e.message)

    def glob(self, globs):
        with self._get_client() as client:
            return client.glob(self._eden_root, globs)

    def getFileInformation(self, files):
        with self._get_client() as client:
            return client.getFileInformation(self._eden_root, files)

    def _flushPendingTransactions(self):
        # If a transaction is currently in progress, make sure it has flushed
        # pending commit data to disk so that eden will be able to access it.
        txn = self._repo.currenttransaction()
        if txn is not None:
            txn.writepending()

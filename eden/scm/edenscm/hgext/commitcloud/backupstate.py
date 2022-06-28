# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

import hashlib
import os

from edenscm.mercurial import error, node as nodemod, util
from edenscm.mercurial.pycompat import encodeutf8

from . import dependencies

FORMAT_VERSION = "v1"


class BackupState(object):
    """Stores what commits have been successfully backed up to the cloud.

    BackupState is not the source of truth, it is a local cache of what has been backed up at the given path.
    """

    prefix = "backedupheads."
    directory = "commitcloud"

    def __init__(self, repo, remotepath, resetlocalstate=False, usehttp=False):
        self.repo = repo
        self.remotepath = remotepath
        self.usehttp = usehttp
        repo.sharedvfs.makedirs(self.directory)
        self.filename = os.path.join(
            self.directory,
            self.prefix + hashlib.sha256(encodeutf8(remotepath)).hexdigest()[0:8],
        )
        self.heads = set()
        if repo.sharedvfs.exists(self.filename) and not resetlocalstate:
            lines = repo.sharedvfs.readutf8(self.filename).splitlines()
            if len(lines) < 2 or lines[0].strip() != FORMAT_VERSION:
                version = lines[0].strip() if len(lines) > 0 else "<empty>"
                repo.ui.debug(
                    "unrecognised backedupheads version '%s', ignoring\n" % version
                )
                self.initfromserver()
                return
            if lines[1].strip() != remotepath:
                repo.ui.debug(
                    "backupheads file is for a different remote ('%s' instead of '%s'), reinitializing\n"
                    % (lines[1].strip(), remotepath)
                )
                self.initfromserver()
                return
            heads = [nodemod.bin(head.strip()) for head in lines[2:]]
            heads = repo.changelog.filternodes(heads, local=True)
            self.heads = set(heads)
        else:
            self.initfromserver()

    def initfromserver(self):
        # Check with the server about all visible commits that we don't already
        # know are backed up.
        repo = self.repo
        remotepath = self.remotepath
        unfi = repo
        unknown = [
            nodemod.hex(n)
            for n in unfi.nodes(
                "not public() - hidden() - (not public() & ::%ln)", self.heads
            )
        ]
        if not unknown:
            return

        if self.usehttp:
            try:
                unknown = [nodemod.bin(node) for node in unknown]
                stream = repo.edenapi.commitknown(unknown)
                nodes = {
                    item["hgid"] for item in stream if item["known"].get("Ok") is True
                }
            except (error.RustError, error.HttpError) as e:
                raise error.Abort(e)
        else:

            def getconnection():
                return repo.connectionpool.get(remotepath)

            nodes = {}
            try:
                nodes = {
                    nodemod.bin(hexnode)
                    for hexnode, backedup in zip(
                        unknown,
                        dependencies.infinitepush.isbackedupnodes(
                            getconnection, unknown
                        ),
                    )
                    if backedup
                }
            except error.RepoError:
                pass

        self.update(nodes)

    @util.propertycache
    def backedup(self):
        unfi = self.repo
        return set(unfi.nodes("not public() & ::%ln", self.heads))

    def _write(self, f):
        f.write(encodeutf8("%s\n" % FORMAT_VERSION))
        f.write(encodeutf8("%s\n" % self.remotepath))
        for h in self.heads:
            f.write(encodeutf8("%s\n" % nodemod.hex(h)))

    def update(self, newnodes, tr=None):
        unfi = self.repo
        # The new backed up heads are the heads of all commits we already knew
        # were backed up plus the newly backed up commits.
        self.heads = list(
            unfi.nodes(
                "heads((not public() & ::%ln) + (not public() & ::%ln))",
                self.heads,
                newnodes,
            )
        )

        if tr is not None:
            tr.addfilegenerator(
                "commitcloudbackedupheads",
                (self.filename,),
                self._write,
                location="shared",
            )
        else:
            with self.repo.sharedvfs.open(self.filename, "w", atomictemp=True) as f:
                self._write(f)

        util.clearcachedproperty(self, "backedup")

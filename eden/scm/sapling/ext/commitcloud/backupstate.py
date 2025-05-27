# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


import contextlib
import os

from sapling import error, node as nodemod, util


FORMAT_VERSION = "v2"


class BackupState:
    """Stores what commits have been successfully backed up to the Commit Cloud.

    BackupState is not the source of truth, it is a local cache of what has been backed up on the server.
    """

    name = "backedupheads.remote"
    directory = "commitcloud"

    def __init__(self, repo, resetlocalstate=False):
        # Don't take repo lock when loading backup state. This way, "cloud backup" etc.
        # won't conflict with stuck commands (e.g. "rebase" waiting for user input).
        if repo.ui.configbool("experimental", "lock-for-backup-state", False):
            cm = repo.lock()
        else:
            cm = contextlib.nullcontext()

        with cm:
            self.repo = repo
            repo.sharedvfs.makedirs(self.directory)
            self.filename = os.path.join(
                self.directory,
                self.name,
            )
            self.heads = set()
            if repo.sharedvfs.exists(self.filename) and not resetlocalstate:
                lines = repo.sharedvfs.readutf8(self.filename).splitlines()
                if len(lines) < 1 or lines[0].strip() != FORMAT_VERSION:
                    version = lines[0].strip() if len(lines) > 0 else "<empty>"
                    repo.ui.debug(
                        "unrecognized backedupheads version '%s', ignoring\n" % version
                    )
                    self.initfromserver()
                    return
                heads = [nodemod.bin(head.strip()) for head in lines[1:]]
                heads = repo.changelog.filternodes(heads, local=True)
                self.heads = set(heads)
            else:
                self.initfromserver()

    def initfromserver(self):
        # Check with the server about all visible commits that we don't already
        # know are backed up.
        repo = self.repo
        unfi = repo
        unknown = [
            nodemod.hex(n)
            for n in unfi.nodes(
                "not public() - hidden() - (not public() & ::%ln)", self.heads
            )
        ]
        if not unknown:
            return

        try:
            unknown = [nodemod.bin(node) for node in unknown]
            stream = repo.edenapi.commitknown(unknown)
            nodes = {item["hgid"] for item in stream if item["known"].get("Ok") is True}
        except (error.UncategorizedNativeError, error.HttpError) as e:
            raise error.Abort(e)

        self.update(nodes)

    @util.propertycache
    def backedup(self):
        unfi = self.repo
        return set(unfi.nodes("not public() & ::%ln", self.heads))

    def _write(self, f):
        f.write(("%s\n" % FORMAT_VERSION).encode())
        for h in self.heads:
            f.write(("%s\n" % nodemod.hex(h)).encode())

    def update(self, newnodes, tr=None):
        unfi = self.repo

        # The new backed up heads are the heads of all commits we already knew
        # were backed up plus the newly backed up commits. The heads are stored as a set.
        self.heads = set(
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

    def filterheads(self, heads):
        # Returns list of missing heads
        return [head for head in heads if head not in self.heads]

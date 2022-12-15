# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""Eden implementation for the dirstatemap class."""

import errno
import stat
from typing import BinaryIO, Dict

import eden.dirstate as eden_dirstate_serializer

from edenscmnative import parsers

from . import dirstate, EdenThriftClient, localrepo, pycompat, ui as ui_mod, util, vfs


MERGE_STATE_NOT_APPLICABLE: int = eden_dirstate_serializer.MERGE_STATE_NOT_APPLICABLE
MERGE_STATE_BOTH_PARENTS: int = eden_dirstate_serializer.MERGE_STATE_BOTH_PARENTS
MERGE_STATE_OTHER_PARENT: int = eden_dirstate_serializer.MERGE_STATE_OTHER_PARENT
DUMMY_MTIME = 0

modefromflag: Dict[str, int] = {
    "": stat.S_IFREG | 0o644,
    "x": stat.S_IFREG | 0o755,
    "l": (stat.S_IFREG if pycompat.iswindows else stat.S_IFLNK) | 0o755,
    "t": stat.S_IFDIR | 0o755,
}


class eden_dirstate_map(dirstate.dirstatemap):
    def __init__(
        self,
        ui: "ui_mod.ui",
        opener: "vfs.abstractvfs",
        root: str,
        thrift_client: "EdenThriftClient.EdenThriftClient",
        repo: "localrepo.localrepository",
    ) -> None:
        super(eden_dirstate_map, self).__init__(ui, opener, root)
        self._thrift_client = thrift_client
        self._repo = repo

    def write(self, st: "BinaryIO", now: int) -> None:  # override
        parents = self.parents()

        # Filter out all "clean" entries when writing. (It's possible we should
        # never allow these to be inserted into self._map in the first place.)
        m = {
            k: (v[0], v[1], v[2])
            for k, v in self._map.items()
            if not (v[0] == "n" and v[2] == MERGE_STATE_NOT_APPLICABLE)
        }
        eden_dirstate_serializer.write(st, parents, m, self.copymap)
        st.close()

        # Inform the edenfs daemon about the parent change.
        # We do not need to flush any pending transaction state here--manifest
        # and changelog data for a transaction is always written to disk before the
        # dirstate is updated.
        self._thrift_client.setHgParents(
            parents[0],
            parents[1],
            need_flush=False,
            p1manifest=self._repo[parents[0]].manifestnode(),
        )
        self._dirtyparents = False
        self.nonnormalset, self.otherparentset = self.nonnormalentries()

    def read(self):  # override
        # ignore HG_PENDING because identity is used only for writing
        self.identity = util.filestat.frompath(self._opener.join(self._filename))

        try:
            fp = self._opendirstatefile()
            try:
                parents, dirstate_tuples, copymap = eden_dirstate_serializer.read(
                    fp, self._filename
                )
            finally:
                fp.close()
        except IOError as e:
            if e.errno != errno.ENOENT:
                raise
            else:
                # If the dirstate file does not exist, then we silently ignore
                # the error because that's what Mercurial's dirstate does.
                return

        if not self._dirtyparents:
            self.setparents(*parents)
        self._map = {
            n: parsers.dirstatetuple(v[0], v[1], v[2], DUMMY_MTIME)
            for n, v in dirstate_tuples.items()
        }
        self.copymap = copymap

    def iteritems(self):
        raise RuntimeError(
            "Should not pycompat.iteritems(invoke) on eden_dirstate_map!"
        )

    def __len__(self):
        raise RuntimeError("Should not invoke __len__ on eden_dirstate_map!")

    def __iter__(self):
        raise RuntimeError("Should not invoke __iter__ on eden_dirstate_map!")

    def keys(self):
        raise RuntimeError("Should not invoke keys() on eden_dirstate_map!")

    def get(self, key, default=None):
        try:
            return self.__getitem__(key)
        except KeyError:
            return default

    def __contains__(self, key):
        return self.get(key) is not None

    def __getitem__(self, filename):
        # type(str) -> parsers.dirstatetuple
        entry = self._map.get(filename)
        if entry is not None:
            return entry

        # edenfs only tracks one parent
        commitctx = self._repo["."]
        node, flag = commitctx._fileinfo(filename)

        mode = modefromflag[flag]
        return parsers.dirstatetuple("n", mode, MERGE_STATE_NOT_APPLICABLE, DUMMY_MTIME)

    def hastrackeddir(self, d):  # override
        # TODO(mbolin): Unclear whether it is safe to hardcode this to False.
        return False

    def hasdir(self, d):  # override
        # TODO(mbolin): Unclear whether it is safe to hardcode this to False.
        return False

    def _insert_tuple(self, filename, state, mode, size, mtime):  # override
        if size != MERGE_STATE_BOTH_PARENTS and size != MERGE_STATE_OTHER_PARENT:
            merge_state = MERGE_STATE_NOT_APPLICABLE
        else:
            merge_state = size

        self._map[filename] = parsers.dirstatetuple(
            state, mode, merge_state, DUMMY_MTIME
        )

    def nonnormalentries(self):
        """Returns a set of filenames."""
        # type() -> Tuple[Set[str], Set[str]]
        nonnorm = set()
        otherparent = set()
        for path, entry in pycompat.iteritems(self._map):
            if entry[0] != "n":
                nonnorm.add(path)
            elif entry[2] == MERGE_STATE_OTHER_PARENT:
                otherparent.add(path)
        return nonnorm, otherparent

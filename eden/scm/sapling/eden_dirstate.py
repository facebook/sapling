# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import stat

import bindings

from . import (
    dirstate,
    eden_dirstate_fs,
    eden_dirstate_map,
    EdenThriftClient as thrift,
    encoding,
    localrepo,
    match as matchmod,
    scmutil,
    ui as ui_mod,
    util,
)
from .eden_dirstate_serializer import MERGE_STATE_BOTH_PARENTS, MERGE_STATE_OTHER_PARENT
from .node import nullid

propertycache = util.propertycache


class eden_dirstate(dirstate.dirstate):
    def __init__(
        self, repo: "localrepo.localrepository", ui: "ui_mod.ui", root: str
    ) -> None:
        self.eden_client = thrift.EdenThriftClient(repo)

        # We should override any logic in dirstate that uses self._validate.
        validate = repo._dirstatevalidate

        try:
            opener = repo.localvfs
        except AttributeError:
            opener = repo.vfs

        super(eden_dirstate, self).__init__(opener, ui, root, validate, repo)

    def _initfs(self) -> None:
        def create_eden_dirstate(ui, opener, root):
            return eden_dirstate_map.eden_dirstate_map(
                ui, opener, root, self.eden_client, self._repo
            )

        self._mapcls = create_eden_dirstate
        self._fs = eden_dirstate_fs.eden_filesystem(self._root, self)

    def __iter__(self):
        # FIXME: This appears to be called by `hg reset`, so we provide a dummy
        # response here, but really, we should outright prohibit this.
        # Most likely, we will have to replace the implementation of `hg reset`.
        return
        # This yield is load bearing - it makes us an iterator.
        yield

    def items(self):  # override
        # This seems like the type of O(repo) operation that should not be
        # allowed. Or if it is, it should be through a separate, explicit
        # codepath.
        #
        # We do provide edeniteritems() for users to iterate through only the
        # files explicitly tracked in the eden dirstate.
        raise NotImplementedError("eden_dirstate.items()")

    def dirs(self):  # override
        raise NotImplementedError("eden_dirstate.dirs()")

    def edeniteritems(self):
        """
        Walk over all items tracked in the eden dirstate.

        This includes non-normal files (e.g., files marked for addition or
        removal), as well as normal files that have merge state information.
        """
        return self._map._items()

    def _p1_ctx(self):
        """Return the context object for the first parent commit."""
        return self._map._repo[self.p1()]

    def _call_match_callbacks(self, match, results1, results2):
        """
        Process all explicit patterns in the match, and call match.bad()
        if necessary

        Returns a dictionary of (path -> mode) for all explicit matches that
        are not already present in the results.  The mode will be None if the
        path does not exist on disk.
        """
        # TODO: We do not currently invoke match.traversedir
        # This is currently only used by `hg purge`, which uses it to remove
        # empty directories.
        # We probably should just build our own Eden-specific version of purge.

        explicit_matches = {}

        for path in sorted(match.files()):
            try:
                if path in results1 or path in results2:
                    continue
                mode = os.lstat(os.path.join(self._root, path)).st_mode
                if stat.S_ISDIR(mode):
                    pass
                elif stat.S_ISREG(mode) or stat.S_ISLNK(mode):
                    explicit_matches[path] = mode
            except OSError as ex:
                # Check to see if this refers to a removed file or directory.
                # Call match.bad() otherwise
                if self._ismissing(path):
                    explicit_matches[path] = None
                else:
                    match.bad(path, ex.strerror)

        return explicit_matches

    def _ismissing(self, path):
        """
        Check to see if this path refers to a deleted file that mercurial
        knows about but that no longer exists on disk.
        """
        # Check to see if the parent commit knows about this path
        parent_mf = self._p1_ctx().manifest()
        if parent_mf.hasdir(path):
            return True

        # Check to see if the non-normal files list knows about this path
        # or any child of this path as a directory name.
        # (This handles the case where an untracked file was added with
        # 'hg add' but then deleted from disk.)
        if path in self._map._tree:
            return True

        dirpath = path + "/"
        for entry in self._map._keys():
            if entry.startswith(dirpath):
                return True

        return False

    def _parent_commit_matches(self, match):
        # Wrap match.bad()
        # We don't want to complain about paths that do not exist in the parent
        # commit but do exist in our non-normal files.
        #
        # However, the default mercurial dirstate.matches() code never invokes
        # bad() at all, so lets just ignore all bad() calls entirely.
        def bad(fn, msg):
            return

        m = matchmod.badmatch(match, bad)
        return self._p1_ctx().matches(m)

    def matches(self, match):  # override
        # Call matches() on the current working directory parent commit
        results = set(self._parent_commit_matches(match))

        # Augument the results with anything modified in the dirstate,
        # to take care of added/removed files.
        for path in self._map._keys():
            if match(path):
                results.add(path)

        return results

    def non_removed_matches(self, match):  # override
        """
        Behaves like matches(), but excludes files that have been removed from
        the dirstate.
        """
        results = set(self._parent_commit_matches(match))

        # Augument the results with anything modified in the dirstate,
        # to take care of added/removed files.
        for path, state in self._map._items():
            if match(path):
                if state[0] == "r":
                    results.discard(path)
                else:
                    results.add(path)

        return results

    def rebuild(self, parent, allfiles, changedfiles=None, exact=False):
        # Ignore the input allfiles parameter, and always rebuild with
        # an empty allfiles list.
        #
        # edenfs itself will track the file changes correctly.
        # We only track merge state and added/removed status in the python
        # dirstate code.
        super(eden_dirstate, self).rebuild(
            parent, allfiles=[], changedfiles=changedfiles, exact=exact
        )

    def normallookup(self, f):  # override
        """Mark a file normal, but possibly dirty."""
        if self.is_merge():
            # if there is a merge going on and the file was either
            # in state 'm' (-1) or coming from other parent (-2) before
            # being removed, restore that state.
            #
            # Note that we intentionally use self._map._tree.get() here
            # rather than self._map.get() to avoid making a thrift call to Eden
            # if this file is already normal.
            entry = self._map._tree.get(f, None)
            if entry is not None:
                status, _mode, merge_state, *_ = entry
                status = bindings.treestate.tohgstate(status)
                if status == "r" and merge_state in (
                    MERGE_STATE_BOTH_PARENTS,
                    MERGE_STATE_OTHER_PARENT,
                ):
                    source = self._map.copymap.get(f)
                    if merge_state == MERGE_STATE_BOTH_PARENTS:
                        self.merge(f)
                    elif merge_state == MERGE_STATE_OTHER_PARENT:
                        self.otherparent(f)
                    if source:
                        self.copy(source, f)
                    return
                if status == "m":
                    return
                if status == "n" and merge_state == MERGE_STATE_OTHER_PARENT:
                    return

        # TODO: Just invoke self.normal() here for now.
        # Our self.status() function always returns an empty list for the first
        # entry of the returned tuple.  (This is the list of files that we're
        # unsure about and need to check on disk.)  Therefore the
        # workingctx._dirstatestatus() code never fixes up entries with the
        # mtime set to -1.
        #
        # Ideally we should replace self.normal() too; we should be able to
        # avoid the filesystem stat call in self.normal() anyway.
        self.normal(f)

    def status(self, *args, **kwargs) -> scmutil.status:
        # If a transaction is currently in progress, make sure it has flushed
        # pending commit data to disk so that eden will be able to access it.
        #
        # This was copied from EdenThriftClient.py to fix "requested parent
        # commit is out-of-date" errors asking Eden for "status". However, it
        # doesn't seem like a good idea to randomly commit the current
        # transaction.
        txn = self._repo.currenttransaction()
        if txn is not None:
            txn.writepending()

        return super().status(*args, **kwargs)

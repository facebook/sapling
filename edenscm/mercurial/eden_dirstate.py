# Copyright (c) 2016-present, Facebook, Inc.
# All Rights Reserved.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import os
import stat

from eden.dirstate import MERGE_STATE_BOTH_PARENTS, MERGE_STATE_OTHER_PARENT

from . import (
    EdenThriftClient as thrift,
    dirstate,
    eden_dirstate_map,
    encoding,
    match as matchmod,
    perftrace,
    policy,
    scmutil,
    util,
)
from .EdenThriftClient import ScmFileStatus
from .i18n import _
from .node import nullid


parsers = policy.importmod("parsers")
propertycache = util.propertycache


class statobject(object):
    """ this is a stat-like object to represent information from eden."""

    __slots__ = ("st_mode", "st_size", "st_mtime")

    def __init__(self, mode=None, size=None, mtime=None):
        self.st_mode = mode
        self.st_size = size
        self.st_mtime = mtime


class eden_dirstate(dirstate.dirstate):
    def __init__(self, repo, ui, root):
        self.eden_client = thrift.EdenThriftClient(repo)

        # We should override any logic in dirstate that uses self._validate.
        validate = repo._dirstatevalidate

        try:
            opener = repo.localvfs
        except AttributeError:
            opener = repo.vfs

        try:
            super(eden_dirstate, self).__init__(opener, ui, root, validate, repo)
        except TypeError:
            sparsematchfn = None
            super(eden_dirstate, self).__init__(
                opener, ui, root, validate, repo, sparsematchfn
            )

        def create_eden_dirstate(ui, opener, root):
            return eden_dirstate_map.eden_dirstate_map(
                ui, opener, root, self.eden_client, repo
            )

        self._mapcls = create_eden_dirstate

    def __iter__(self):
        # FIXME: This appears to be called by `hg reset`, so we provide a dummy
        # response here, but really, we should outright prohibit this.
        # Most likely, we will have to replace the implementation of `hg reset`.
        return
        yield

    def iteritems(self):  # override
        # This seems like the type of O(repo) operation that should not be
        # allowed. Or if it is, it should be through a separate, explicit
        # codepath.
        #
        # We do provide edeniteritems() for users to iterate through only the
        # files explicitly tracked in the eden dirstate.
        raise NotImplementedError("eden_dirstate.iteritems()")

    def dirs(self):  # override
        raise NotImplementedError("eden_dirstate.dirs()")

    def edeniteritems(self):
        """
        Walk over all items tracked in the eden dirstate.

        This includes non-normal files (e.g., files marked for addition or
        removal), as well as normal files that have merge state information.
        """
        return self._map._map.iteritems()

    def _p1_ctx(self):
        """Return the context object for the first parent commit."""
        return self._map._repo.unfiltered()[self.p1()]

    # Code paths that invoke dirstate.walk()
    # - hg add
    #   unknown=True, ignored=False, full=False
    #   - only cares about returned paths
    # - hg perfwalk (contrib/perf.py)
    #   unknown=True, ignored=False
    #   - only cares about returned paths
    # - hg grep (hgext/tweakdefaults.py)
    #   unknown=False, ignored=False
    #   - cares about returned paths, whether exists, and is symlink or not
    # - committablectx.walk()
    #   unknown=True, ignored=False
    #   - only cares about returned paths
    # - mercurial/scmutil.py: _interestingfiles()
    #   unknown=True, ignored=False, full=False
    #   hg addremove
    #   - cares about returned paths, dirstate status (which it has to
    #     re-lookup), and whether they exist on disk or not
    #
    # Code paths that invoke context.walk()
    # - mercurial/cmdutil.py:
    #   - hg cat
    #   - hg cp
    #   - hg revert
    # - hg annotate (mercurial/commands.py)
    # - mercurial/debugcommands.py:
    #   - hg debugfilerevision
    #   - hg debugpickmergetool
    #   - hg debugrename
    #   - hg debugwalk
    # - mercurial/fileset.py (_buildsubset)
    # - hgext/catnotate.py
    # - hgext/fastannotate/commands.py
    # - hgext/sparse.py
    # - hgext/remotefilelog/__init__.py
    #
    # Code paths that invoke scmutil._interestingfiles()
    # - scmutil.addremove()
    # - scmutil.marktouched()
    #
    # - full is primarily used by fsmonitor extension
    # - I haven't seen any code path that calls with ignored=True
    #
    # match callbacks:
    # - bad: called for file/directory patterns that don't match anything
    # - traversedir: used by `hg purge` (hgext/purge.py) to purge empty
    #   directories
    #   - we potentially should just implement purge inside Eden
    #
    def walk(self, match, unknown, ignored, full=True):  # override
        """
        Walk recursively through the directory tree, finding all files
        matched by match.

        If full is False, maybe skip some known-clean files.

        Return a dict mapping filename to stat-like object
        """
        with perftrace.trace("Get EdenFS Status"):
            perftrace.traceflag("walk")
            edenstatus = self.eden_client.getStatus(
                self.p1(), list_ignored=ignored
            ).entries

        nonnormal = self._map._map

        def get_stat(path):
            try:
                return os.lstat(os.path.join(self._root, path))
            except OSError:
                return None

        # Store local variables for the status states, so they are cheaper
        # to access in the loop below.  (It's somewhat unfortunate that python
        # make this necessary.)
        MODIFIED = ScmFileStatus.MODIFIED
        REMOVED = ScmFileStatus.REMOVED
        ADDED = ScmFileStatus.ADDED
        IGNORED = ScmFileStatus.IGNORED

        results = {}
        for path, code in edenstatus.iteritems():
            if not match(path):
                continue

            # TODO: It would probably be better to update the thrift APIs to
            # return the file status information, so we don't have to call
            # os.lstat() here.  Most callers only really care about whether the
            # file exists and if it is a symlink or a regular file.
            if code == MODIFIED:
                results[path] = get_stat(path)
            elif code == ADDED:
                # If unknown is False, we still want to report files explicitly
                # marked as added in the dirstate.  We'll handle that case
                # below when walking over the nonnormal list.
                if unknown:
                    results[path] = get_stat(path)
            elif code == IGNORED:
                # Eden should only return IGNORED results when ignored is True,
                # so just go ahead and add this path to the results
                results[path] = get_stat(path)
            elif code == REMOVED:
                results[path] = None
            else:
                raise RuntimeError("Unexpected status code: %s" % code)

        for path, entry in nonnormal.iteritems():
            if path in results:
                continue
            if not match(path):
                continue
            results[path] = get_stat(path)

        if full:
            parent_mf = self._p1_ctx().manifest()
            for path, flags in parent_mf.matches(match).iteritems():
                if path in edenstatus or path in nonnormal:
                    continue
                if flags == "l":
                    mode = stat.S_IFLNK | 0o777
                elif flags == "x":
                    mode = stat.S_IFREG | 0o755
                else:
                    mode = stat.S_IFREG | 0o644
                # Pretty much all of the callers of walk() only care about
                # the st_mode field.
                results[path] = statobject(mode=mode, size=0, mtime=0)

        explicit_matches = self._call_match_callbacks(match, results, ())
        for path, mode in explicit_matches.iteritems():
            if mode is None:
                results[path] = None
            else:
                results[path] = statobject(mode=mode, size=0, mtime=0)

        return results

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
                    match.bad(path, encoding.strtolocal(ex.strerror))

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
        if path in self._map._map:
            return True

        dirpath = path + "/"
        for entry in self._map._map:
            if entry.startswith(dirpath):
                return True

        return False

    def status(self, match, ignored, clean, unknown):  # override
        with perftrace.trace("Get EdenFS Status"):
            perftrace.traceflag("status")
            edenstatus = self.eden_client.getStatus(
                self.p1(), list_ignored=ignored
            ).entries

        nonnormal_copy = self._map.create_clone_of_internal_map()

        # If the caller also wanted us to return clean files,
        # find all matching files from the current commit manifest.
        # If they are not in the eden status results or the dirstate
        # non-normal list then they must be clean.
        clean_files = []
        if clean:
            for path in self._parent_commit_matches(match):
                if path not in edenstatus and path not in nonnormal_copy:
                    clean_files.append(path)

        # Store local variables for the status states, so they are cheaper
        # to access in the loop below.  (It's somewhat unfortunate that python
        # make this necessary.)
        MODIFIED = ScmFileStatus.MODIFIED
        REMOVED = ScmFileStatus.REMOVED
        ADDED = ScmFileStatus.ADDED
        IGNORED = ScmFileStatus.IGNORED

        # Process the modified file list returned by Eden.
        # We must merge it with our list of non-normal files to compute
        # the removed/added lists correctly.
        modified_files = []
        added_files = []
        removed_files = []
        deleted_files = []
        unknown_files = []
        ignored_files = []
        for path, code in edenstatus.iteritems():
            if not match(path):
                continue

            if code == MODIFIED:
                # It is possible that the user can mark a file for removal, but
                # then modify it. If it is marked for removal, it should be
                # reported as such by `hg status` even though it is still on
                # disk.
                dirstate = nonnormal_copy.pop(path, None)
                if dirstate and dirstate[0] == "r":
                    removed_files.append(path)
                else:
                    modified_files.append(path)
            elif code == REMOVED:
                # If the file no longer exits, we must check to see whether the
                # user explicitly marked it for removal.
                dirstate = nonnormal_copy.pop(path, None)
                if dirstate and dirstate[0] == "r":
                    removed_files.append(path)
                else:
                    deleted_files.append(path)
            elif code == ADDED:
                dirstate = nonnormal_copy.pop(path, None)
                if dirstate:
                    state = dirstate[0]
                    if state == "a" or (
                        state == "n" and dirstate[2] == MERGE_STATE_OTHER_PARENT
                    ):
                        added_files.append(path)
                    else:
                        unknown_files.append(path)
                else:
                    unknown_files.append(path)
            elif code == IGNORED:
                # Although Eden may think the file should be ignored as per
                # .gitignore, it is possible the user has overridden that
                # default behavior by marking it for addition.
                dirstate = nonnormal_copy.pop(path, None)
                if dirstate and dirstate[0] == "a":
                    added_files.append(path)
                else:
                    ignored_files.append(path)
            else:
                raise RuntimeError("Unexpected status code: %s" % code)

        # Process any remaining files in our non-normal set that were
        # not reported as modified by Eden.
        for path, entry in nonnormal_copy.iteritems():
            if not match(path):
                continue

            state = entry[0]
            if state == "m":
                if entry[2] == 0:
                    self._ui.warn(
                        _(
                            "Unexpected Nonnormal file " + path + " has a "
                            "merge state of NotApplicable while its has been "
                            'marked as "needs merging".'
                        )
                    )
                else:
                    modified_files.append(path)
            elif state == "a":
                try:
                    mode = os.lstat(os.path.join(self._root, path)).st_mode
                    if stat.S_ISREG(mode) or stat.S_ISLNK(mode):
                        added_files.append(path)
                    else:
                        deleted_files.append(path)
                except OSError:
                    deleted_files.append(path)
            elif state == "r":
                removed_files.append(path)

        # Invoked the match callback functions.
        explicit_matches = self._call_match_callbacks(match, edenstatus, nonnormal_copy)
        for path in explicit_matches:
            # Explicit matches that aren't already present in our results
            # were either skipped because they are ignored or they are clean.
            # Check to figure out which is the case.
            if clean:
                ignored_files.append(path)
            elif path in self._p1_ctx():
                clean_files.append(path)
            else:
                ignored_files.append(path)

        status = scmutil.status(
            modified_files,
            added_files,
            removed_files,
            deleted_files,
            unknown_files,
            ignored_files,
            clean_files,
        )

        return status

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
        for path in self._map._map.keys():
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
        for path, state in self._map._map.items():
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
        if self._pl[1] != nullid:
            # if there is a merge going on and the file was either
            # in state 'm' (-1) or coming from other parent (-2) before
            # being removed, restore that state.
            #
            # Note that we intentionally use self._map._map.get() here
            # rather than self._map.get() to avoid making a thrift call to Eden
            # if this file is already normal.
            entry = self._map._map.get(f)
            if entry is not None:
                status, mode, merge_state = entry
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

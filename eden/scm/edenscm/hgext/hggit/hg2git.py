# This file contains code dealing specifically with converting Mercurial
# repositories to Git repositories. Code in this file is meant to be a generic
# library and should be usable outside the context of hg-git or an hg command.

import os
import stat

# pyre-fixme[21]: Could not find `compat`.
import compat
import dulwich.objects as dulobjs

# pyre-fixme[21]: Could not find `util`.
import util
from edenscm.mercurial import util as hgutil
from edenscm.mercurial.i18n import _


def audit_git_path(ui, path):
    r"""Check for path components that case-fold to .git.

    >>> class fakeui(object):
    ...     def configbool(*args):
    ...         return False
    ...     def warn(self, s):
    ...         print s
    >>> u = fakeui()
    >>> audit_git_path(u, 'foo/git~100/wat')
    warning: path 'foo/git~100/wat' contains a dangerous path component.
    It may not be legal to check out in Git.
    It may also be rejected by some git server configurations.
    <BLANKLINE>
    >>> audit_git_path(u, u'foo/.gi\u200ct'.encode('utf-8'))
    warning: path 'foo/.gi\xe2\x80\x8ct' contains a dangerous path component.
    It may not be legal to check out in Git.
    It may also be rejected by some git server configurations.
    <BLANKLINE>
    >>> audit_git_path(u, 'this/is/safe')
    """
    dangerous = False
    for c in path.split(os.path.sep):
        if compat.hfsignoreclean(c) == ".git":
            dangerous = True
            break
        elif "~" in c:
            base, tail = c.split("~", 1)
            if tail.isdigit() and base.upper().startswith("GIT"):
                dangerous = True
                break
    if dangerous:
        if compat.config(ui, "bool", "git", "blockdotgit"):
            raise hgutil.Abort(
                _("Refusing to export likely-dangerous path %r") % path,
                hint=_(
                    "If you need to continue, read about CVE-2014-9390 and "
                    "then set '[git] blockdotgit = false' in your hgrc."
                ),
            )
        ui.warn(
            _(
                "warning: path %r contains a dangerous path component.\n"
                "It may not be legal to check out in Git.\n"
                "It may also be rejected by some git server configurations.\n"
            )
            % path
        )


class GitTreeDict(object):
    """A mutable structure pretending to be a fullpath->tree dictionary of all the trees
    in the given commit.

    Trees are lazily loaded as needed, and any path that is set has its parents
    automatically loaded/created as well.
    """

    def __init__(self, store, commit):
        self.store = store
        self.trees = {}
        self.trees[""] = store[commit.tree] if commit is not None else dulobjs.Tree()

    def __getitem__(self, path):
        value = self.get(path)
        if value is None:
            raise KeyError("no path %s" % path)
        return value

    def get(self, path, default=None):
        value = self.trees.get(path)
        if value is None:
            # It's not in our cache, so let's find the parent so we can add this
            # entry.
            if path == "":
                raise KeyError("missing required '' root")
            base, name = os.path.split(path)
            parent = self.get(base)
            if parent is None or name not in parent:
                return default

            # Load the missing child tree
            child_mode, child_id = parent[name]
            if child_mode != stat.S_IFDIR:
                raise KeyError("trying to set non-tree child %s" % path)

            value = self.store[child_id]
            self.trees[path] = value

        return value

    def __setitem__(self, path, value):
        base, name = os.path.split(path)
        parent = self.get(base)
        if parent is None:
            # Empty trees will be filled during finalization (see comment below)
            self[base] = dulobjs.Tree()
        # In an ideal world we would assign the child to the parent here, but
        # parents store a reference to the child's id which we don't have since
        # the tree is still being mutated. Instead we create empty parents where
        # needed, and later in _populate_tree_entries we attach all the children
        # to parents bottom-up.
        self.trees[path] = value

    def __delitem__(self, path):
        if path == "":
            raise KeyError("cannot delete root path")
        del self.trees[path]

    def setdefault(self, path, default):
        value = self.get(path)
        if value is None:
            value = default
            self[path] = value

        return value


class IncrementalChangesetExporter(object):
    """Incrementally export Mercurial changesets to Git trees.

    The purpose of this class is to facilitate Git tree export that is more
    optimal than brute force.

    A "dumb" implementations of Mercurial to Git export would iterate over
    every file present in a Mercurial changeset and would convert each to
    a Git blob and then conditionally add it to a Git repository if it didn't
    yet exist. This is suboptimal because the overhead associated with
    obtaining every file's raw content and converting it to a Git blob is
    not trivial!

    This class works around the suboptimality of brute force export by
    leveraging the information stored in Mercurial - the knowledge of what
    changed between changesets - to only export Git objects corresponding to
    changes in Mercurial. In the context of converting Mercurial repositories
    to Git repositories, we only export objects Git (possibly) hasn't seen yet.
    This prevents a lot of redundant work and is thus faster.

    Callers instantiate an instance of this class against a mercurial.localrepo
    instance. They then associate it with a specific changesets by calling
    update_changeset(). On each call to update_changeset(), the instance
    computes the difference between the current and new changesets and emits
    Git objects that haven't yet been encountered during the lifetime of the
    class instance. In other words, it expresses Mercurial changeset deltas in
    terms of Git objects. Callers then (usually) take this set of Git objects
    and add them to the Git repository.

    This class only emits Git blobs and trees, not commits.

    The tree calculation part of this class is essentially a reimplementation
    of dulwich.index.commit_tree. However, since our implementation reuses
    Tree instances and only recalculates SHA-1 when things change, we are
    more efficient.
    """

    def __init__(self, hg_repo, start_ctx, git_store, git_commit):
        """Create an instance against a mercurial.localrepo.

        start_ctx: the context for a Mercurial commit that has a Git
                   equivalent, passed in as git_commit. The incremental
                   computation will be started from this commit.
        git_store: the Git object store the commit comes from.

        start_ctx can be repo[nullid], in which case git_commit should be None.
        """
        self._hg = hg_repo

        # Our current revision's context.
        self._ctx = start_ctx

        # Path to dulwich.objects.Tree.
        self._dirs = GitTreeDict(git_store, git_commit)

        # Mercurial file nodeid to Git blob SHA-1. Used to prevent redundant
        # blob calculation.
        self._blob_cache = {}

    @property
    def root_tree_sha(self):
        """The SHA-1 of the root Git tree.

        This is needed to construct a Git commit object.
        """
        return self._dirs[""].id

    def update_changeset(self, newctx):
        """Set the tree to track a new Mercurial changeset.

        This is a generator of 2-tuples. The first item in each tuple is a
        dulwich object, either a Blob or a Tree. The second item is the
        corresponding Mercurial nodeid for the item, if any. Only blobs will
        have nodeids. Trees do not correspond to a specific nodeid, so it does
        not make sense to emit a nodeid for them.

        When exporting trees from Mercurial, callers typically write the
        returned dulwich object to the Git repo via the store's add_object().

        Some emitted objects may already exist in the Git repository. This
        class does not know about the Git repository, so it's up to the caller
        to conditionally add the object, etc.

        Emitted objects are those that have changed since the last call to
        update_changeset. If this is the first call to update_chanageset, all
        objects in the tree are emitted.
        """
        # Our general strategy is to accumulate dulwich.objects.Blob and
        # dulwich.objects.Tree instances for the current Mercurial changeset.
        # We do this incremental by iterating over the Mercurial-reported
        # changeset delta. We rely on the behavior of Mercurial to lazy
        # calculate a Tree's SHA-1 when we modify it. This is critical to
        # performance.

        # In theory we should be able to look at changectx.files(). This is
        # *much* faster. However, it may not be accurate, especially with older
        # repositories, which may not record things like deleted files
        # explicitly in the manifest (which is where files() gets its data).
        # The only reliable way to get the full set of changes is by looking at
        # the full manifest. And, the easy way to compare two manifests is
        # localrepo.status().
        modified, added, removed = self._hg.status(self._ctx, newctx)[0:3]

        # We track which directories/trees have modified in this update and we
        # only export those.
        dirty_trees = set()

        for path in removed:
            self._remove_path(path, dirty_trees)

        # For every file that changed or was added, we need to calculate the
        # corresponding Git blob and its tree entry. We emit the blob
        # immediately and update trees to be aware of its presence.
        for path in set(modified) | set(added):
            audit_git_path(self._hg.ui, path)
            d = os.path.dirname(path)
            tree = self._dirs.setdefault(d, dulobjs.Tree())
            dirty_trees.add(d)

            fctx = newctx[path]

            func = IncrementalChangesetExporter.tree_entry
            entry, blob = func(fctx, self._blob_cache)
            if blob is not None:
                yield (blob, fctx.filenode())

            tree.add(*entry)

        # Now that all the trees represent the current changeset, recalculate
        # the tree IDs and emit them. Note that we wait until now to calculate
        # tree SHA-1s. This is an important difference between us and
        # dulwich.index.commit_tree(), which builds new Tree instances for each
        # series of blobs.
        for obj in self._populate_tree_entries(dirty_trees):
            yield (obj, None)

        self._ctx = newctx

    def _remove_path(self, path, dirty_trees):
        """Remove a path (file or git link) from the current changeset.

        If the tree containing this path is empty, it might be removed."""
        d = os.path.dirname(path)
        tree = self._dirs.get(d, dulobjs.Tree())

        del tree[os.path.basename(path)]
        dirty_trees.add(d)

        # If removing this file made the tree empty, we should delete this
        # tree. This could result in parent trees losing their only child
        # and so on.
        if not len(tree):
            self._remove_tree(d)
        else:
            self._dirs[d] = tree

    def _remove_tree(self, path):
        """Remove a (presumably empty) tree from the current changeset.

        A now-empty tree may be the only child of its parent. So, we traverse
        up the chain to the root tree, deleting any empty trees along the way.
        """
        try:
            del self._dirs[path]
        except KeyError:
            return

        # Now we traverse up to the parent and delete any references.
        if path == "":
            return

        basename = os.path.basename(path)
        parent = os.path.dirname(path)
        while True:
            tree = self._dirs.get(parent, None)

            # No parent entry. Nothing to remove or update.
            if tree is None:
                return

            try:
                del tree[basename]
            except KeyError:
                return

            if len(tree):
                return

            # The parent tree is empty. Se, we can delete it.
            del self._dirs[parent]

            if parent == "":
                return

            basename = os.path.basename(parent)
            parent = os.path.dirname(parent)

    def _populate_tree_entries(self, dirty_trees):
        self._dirs.setdefault("", dulobjs.Tree())

        for dirty in list(dirty_trees):
            parent = os.path.dirname(dirty)

            while parent != "":
                if parent in dirty_trees:
                    break

                dirty_trees.add(parent)
                parent = os.path.dirname(parent)

        # The root tree is always dirty but doesn't always get updated.
        dirty_trees.add("")

        # We only need to recalculate and export dirty trees.
        for d in sorted(dirty_trees, key=len, reverse=True):
            # Only happens for deleted directories.
            try:
                tree = self._dirs[d]
            except KeyError:
                continue

            yield tree

            if d == "":
                continue

            parent_tree = self._dirs[os.path.dirname(d)]

            # Accessing the tree's ID is what triggers SHA-1 calculation and is
            # the expensive part (at least if the tree has been modified since
            # the last time we retrieved its ID). Also, assigning an entry to a
            # tree (even if it already exists) invalidates the existing tree
            # and incurs SHA-1 recalculation. So, it's in our interest to avoid
            # invalidating trees. Since we only update the entries of dirty
            # trees, this should hold true.
            parent_tree[os.path.basename(d)] = (stat.S_IFDIR, tree.id)

    @staticmethod
    def tree_entry(fctx, blob_cache):
        """Compute a dulwich TreeEntry from a filectx.

        A side effect is the TreeEntry is stored in the passed cache.

        Returns a 2-tuple of (dulwich.objects.TreeEntry, dulwich.objects.Blob).
        """
        blob_id = blob_cache.get(fctx.filenode(), None)
        blob = None

        if blob_id is None:
            blob = dulobjs.Blob.from_string(fctx.data())
            blob_id = blob.id
            blob_cache[fctx.filenode()] = blob_id

        flags = fctx.flags()

        if "l" in flags:
            mode = 0o120000
        elif "x" in flags:
            mode = 0o100755
        else:
            mode = 0o100644

        return (dulobjs.TreeEntry(os.path.basename(fctx.path()), mode, blob_id), blob)

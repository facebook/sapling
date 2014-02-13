# This file contains code dealing specifically with converting Mercurial
# repositories to Git repositories. Code in this file is meant to be a generic
# library and should be usable outside the context of hg-git or an hg command.

import os
import stat

import dulwich.objects as dulobjs
import mercurial.node
import mercurial.context

import util

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

    def __init__(self, hg_repo):
        """Create an instance against a mercurial.localrepo."""
        self._hg = hg_repo

        # Our current revision's context.
        self._ctx = mercurial.context.changectx(hg_repo, 'null')

        # Path to dulwich.objects.Tree.
        self._dirs = {}

        # Mercurial file nodeid to Git blob SHA-1. Used to prevent redundant
        # blob calculation.
        self._blob_cache = {}

    @property
    def root_tree_sha(self):
        """The SHA-1 of the root Git tree.

        This is needed to construct a Git commit object.
        """
        return self._dirs[''].id

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

        # We first process file removals so we can prune dead trees.
        for path in removed:
            self._remove_path(path, dirty_trees)

        # For every file that changed or was added, we need to calculate the
        # corresponding Git blob and its tree entry. We emit the blob
        # immediately and update trees to be aware of its presence.
        for path in set(modified) | set(added):
            # Handle special Mercurial paths.
            if path == '.hgsubstate':
                self._handle_subrepos(newctx, dirty_trees)
                continue

            if path == '.hgsub':
                continue

            d = os.path.dirname(path)
            tree = self._dirs.setdefault(d, dulobjs.Tree())
            dirty_trees.add(d)

            fctx = newctx[path]

            entry, blob = IncrementalChangesetExporter.tree_entry(fctx,
                self._blob_cache)
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
        if path == '':
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

            if parent == '':
                return

            basename = os.path.basename(parent)
            parent = os.path.dirname(parent)

    def _populate_tree_entries(self, dirty_trees):
        self._dirs.setdefault('', dulobjs.Tree())

        # Fill in missing directories.
        for path in self._dirs.keys():
            parent = os.path.dirname(path)

            while parent != '':
                parent_tree = self._dirs.get(parent, None)

                if parent_tree is not None:
                    break

                self._dirs[parent] = dulobjs.Tree()
                parent = os.path.dirname(parent)

        for dirty in list(dirty_trees):
            parent = os.path.dirname(dirty)

            while parent != '':
                if parent in dirty_trees:
                    break

                dirty_trees.add(parent)
                parent = os.path.dirname(parent)

        # The root tree is always dirty but doesn't always get updated.
        dirty_trees.add('')

        # We only need to recalculate and export dirty trees.
        for d in sorted(dirty_trees, key=len, reverse=True):
            # Only happens for deleted directories.
            try:
                tree = self._dirs[d]
            except KeyError:
                continue

            yield tree

            if d == '':
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

    def _handle_subrepos(self, newctx, dirty_trees):
        def parse_subrepos(ctx):
            sub = util.OrderedDict()
            if '.hgsub' in ctx:
                sub = util.parse_hgsub(ctx['.hgsub'].data().splitlines())
            substate = util.OrderedDict()
            if '.hgsubstate' in ctx:
                substate = util.parse_hgsubstate(
                    ctx['.hgsubstate'].data().splitlines())
            return sub, substate

        sub, substate = parse_subrepos(self._ctx)
        newsub, newsubstate = parse_subrepos(newctx)

        # For each path, the logic is described by the following table. 'no'
        # stands for 'the subrepo doesn't exist', 'git' stands for 'git
        # subrepo', and 'hg' stands for 'hg or other subrepo'.
        #
        #  old  new  |  action
        #   *   git  |   link    (1)
        #  git   hg  |  delete   (2)
        #  git   no  |  delete   (3)
        #
        # All other combinations are 'do nothing'.
        #
        # git links without corresponding submodule paths are stored as subrepos
        # with a substate but without an entry in .hgsub.

        def isgit(sub, path):
            return path not in sub or sub[path].startswith('[git]')

        for path, sha in substate.iteritems():
            if not isgit(sub, path):
                # old = hg -- will be handled in next loop
                continue
            # old = git
            if path not in newsubstate or not isgit(newsub, path):
                # new = hg or no, case (2) or (3)
                self._remove_path(path, dirty_trees)

        for path, sha in newsubstate.iteritems():
            if not isgit(newsub, path):
                # new = hg or no; the only cases we care about are handled above
                continue

            # case (1)
            d = os.path.dirname(path)
            dirty_trees.add(d)
            tree = self._dirs.setdefault(d, dulobjs.Tree())
            tree.add(os.path.basename(path), dulobjs.S_IFGITLINK, sha)

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

        if 'l' in flags:
            mode = 0120000
        elif 'x' in flags:
            mode = 0100755
        else:
            mode = 0100644

        return (dulobjs.TreeEntry(os.path.basename(fctx.path()), mode, blob_id),
                blob)


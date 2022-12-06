Visibility
==========

Refer to https://sapling-scm.com/docs/internals/visibility-and-mutation

.. note:: 

   This document describes new-style visibility and narrow-heads.

The data storage of Mercurial repositories can contain commits other than the
ones that are currently visible in the repository.  For example, when a commit
is amended, the old commit is hidden.

There are three categories of commits: public, draft, and hidden.

* Mercurial considers any commit that is an ancestor of a non-scratch remote
  bookmark to be **public**.

* For draft commits, Mercurial explicitly tracks visible draft heads, which are
  the heads of all draft stacks.  These are kept in ``.hg/store/visibleheads``
  and in the metalog.  Commits which are not public, but are ancestors of draft
  heads, are **draft**.

* All other commits are **hidden**.

Hidden commits are still accessible via commit hashes or revsets like
``predecessors()``, however revsets like ``all()``, ``children()`` and
``descendants()`` will not expose hidden commits by default.  Accessing a
hidden commit used to be an error before narrow-heads. 

A commit is visible, and so shows up in commands like ``smartlog`` if it is an
ancestor of:

* Any non-scratch remote bookmark.  These are public commits.

* Any visible draft head.  These are draft commits.

* The current working copy.  Checking out a hidden commit temporarily makes it
  and its ancestors visible.

Mutation
========

Refers to https://sapling-scm.com/docs/internals/visibility-and-mutation

When commits are amended or rebased, Mercurial creates a new commit and hides
the old one.  Mercurial also tracks this mutation relationship between these
commits.

.. note::

   Mutation tracking is based on changeset evolution and obsolescence markers with an important
   distinction: obsolescence markers influence commit visibility, whereas mutation records do not.
   Visibility of commits is tracked separately (See Visibility_).

Mutation tracking is enabled by config:

::

   [mutation]
   enabled = true

Mutation Records
----------------

When a commit is created by mutating one or more existing commits, Mercurial records the following in the mutation store for the repository:

* Successor - the changeset ID of the newly created commit.
* Predecessors - the changeset IDs of the commits that were mutated to create this commit.
* Splits - the changeset IDs of other commits that were also created by splitting the predecessors.
* The name of the mutation operation.
* The user name of the user performing the mutation.
* The Time and Time Zone of the mutation operation.

Most operations (e.g. amend and rebase) will have a single predecessor and no splits.

Mutation records can have multiple predecessors if:

* The operation was a fold, or fold-like operation (e.g. during histedit)
* The operation was a rebase of two or more commits that were already related
  by a mutation record.  The mutation record will be copied with the new
  successor having the old successor and the rebased predecessors as its
  predecessors.

If a commit is split into multiple commits, the successor is the top of the
stack of split commits, and the other commits in the stack are recorded as
splits.

Mutation records can also store extra key-value pairs, but these are currently not used.

Example
~~~~~~~

::

                             .--> F --rebase
                            /     |      \
     B  --rebase--> D  --split--> E --.   '--> H
     |              |             |    \       |
     A  --amend---> C             C --fold---> G
     |              |             |            |
     Z              Z             Z            Z


========= ============ ====== =========
Successor Predecessors Splits Operation
========= ============ ====== =========
C         A                   amend
--------- ------------ ------ ---------
D         B                   rebase
--------- ------------ ------ ---------
F         D            E      split
--------- ------------ ------ ---------
G         C E                 fold
--------- ------------ ------ ---------
H         F                   rebase
========= ============ ====== =========

Successor Uniqueness
~~~~~~~~~~~~~~~~~~~~

Mutation records are associated with the successor commit, and there is expected
to be at most one mutation record for a commit: the mutation record describes
how the commit came to exist.

It is possible for some mutation commands to result in a commit that is an exact
duplicate of an earlier commit.  When this happens, the commit would ordinarily have
the same commit hash as the earlier commit, which would form a cycle in the mutation
graph.

To prevent these cycles from occurring, the mutation data can also be recorded into
the commit extras by setting the configuration option:

::

   [mutation]
   record = true

This adds new extras like ``mutdate``, ``mutop``, ``mutpred`` and ``mutuser`` to the
successor commit which ensures its hash is unique.

Successor Visibility and Undoing Mutation
-----------------------------------------

Mutation records only have effect if the successor commit, or one of its
eventual successor commits, is visible.  If both a predecessor commit and a
successor commit are visible, then the predecessor commit is marked as obsolete,
replaced by the successor commit.  However, if the successor commit is then
hidden, the predecessor commit is automatically revived.

This allows mutation operations to be undone by hiding the successor commit and
making the predecessor commit visible again.

Combining this with new-style visibility tracking, a set of commits can always
be restored to the same visibility and mutation state by restoring the set of
visible heads to how they were previously.

Mutation Record Exchange
------------------------

As mutation records do not affect commit visibility, and only have an effect if the
successor commit is visible, it is always safe to share mutation records between
repositories.

When syncing draft commits between repositories, we can ensure that the mutation state
is kept in sync by transmitting the mutation records associated with all visible draft
commits and their predecessors to the other repository.  The destination repository
can ignore all mutation records for which a mutation record already exists with the same
successor, as mutation records should be unique per successor.

.. _Visibility: Visibility

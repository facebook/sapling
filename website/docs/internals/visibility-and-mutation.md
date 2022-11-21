import {SLCommand} from '@site/elements'

# Visibility and mutation

The way Sapling tracks which commits are visible to an individual developer, and how it tracks the ways in which commits have been mutated, differs from other source control systems.  Designing a system to allow mutations of the otherwise immutable commit graph brings its own scaling challenges.  This document describes the way these have been addressed in Sapling.

## Commit visibility

A Sapling repository can contain more commits than the commits you're currently working on.  For example, when you amend a commit, the old version of the commit remains in the repository.  You can still access it directly using its hash, but otherwise the commit is *hidden*.

Sapling tracks visibility of commits in a couple of ways:

* Any commit that is an ancestor of a remote bookmark is visible.  These commits are public and cannot be modified.
* Sapling also tracks a set of visible heads.  Any commit that is an ancestor of a visible head is visible.  These commits are draft, and may be modified by commands like <SLCommand name="amend" />.  When a commit is created or modified, Sapling automatically removes any old versions of the commits from this set, and adds the new ones to it.

This is similar to how Git tracks which commits are reachable in the repository using the local and remote branches, except that Sapling maintains your local branches for you automatically.

While most visibility operations are automatic, you can also manually hide and unhide commits using the <SLCommand name="hide" /> and <SLCommand name="unhide" /> commands.

In order to scale to thousands of developers contributing to the same repository, commit visibility is entirely local.  Which commits are visible to you are not shared with other developers, so if you hide a commit, it is only hidden for you.

## Commit mutation

Sapling tracks whenever commits are modified using commands like rebase or amend.  The records of these changes are called *mutations*.

This is similar to some parts of the *Evolve* extension of Mercurial, however it is designed to be more lightweight to allow scaling to very large repositories with millions of modified commits.

A mutation record is a record of how a modified commit came to exist.  For example, if you run <SLCommand name="amend" /> on commit A to produce commit B, Sapling will store a record that says "commit B was created by amending commit A".  It will also record who performed the mutation, and the timestamp.

For the most part, mutation records are purely informational.  They affect Sapling operations in two ways:

* In smartlog, if both a commit that has been modified and its modified version are visible, the earlier commit will show as *obsolete* and the latest version's hash will be shown next to the commit, along with whatever operation caused the modification.
* When restacking commits after modifying a commit in the middle of stack, Sapling will use the mutation information of a commit's parent to determine the latest commits that should be used as the destination for each restack.

You can also find earlier and later versions of a commit using the `predecessors` and `successors` revsets.

When used with the Sapling server, commit mutation information can be shared with other developers working on the same commits. When pushing or pulling draft commits to or from the server, Sapling includes records covering the full mutation history of the pushed or pulled commits.  It's not necessary for the older commits themselves to be shared, as Sapling can skip over commits in the history that are not present in the local repository.  In order for this to scale to thousands of developers making millions of changes, Sapling only considers mutation records in the mutation history of the draft commits that you are currently working on, i.e., those that are visible in your local repository.

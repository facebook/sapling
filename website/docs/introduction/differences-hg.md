---
sidebar_position: 50
---
# Differences from Mercurial

While Sapling began 10 years ago as a variant of Mercurial, it has evolved into its own source control system and has many incompatible differences with Mercurial.

The list of differences below is not comprehensive, nor is it meant to be a competitive comparison of Mercurial and Sapling. It just highlights some interesting differences for curious people who are already familiar with Mercurial. Many of the differences from Git also apply to Mercurial and [that list](./differences-git.md) should be referred to as well. Sapling has substantial scaling, implementation, and format differences as well that are not covered here.

#### Sapling has different default behavior and options for many commands.
Sapling removes or changes the behavior of a number of Mercurial commands in order to make the behavior more consistent with modern expectations. For instance, in Sapling ‘sl log’ by default shows the history from your current commit. In Mercurial `hg log` shows the history of the entire repository at once.

Features that are off by default in Mercurial, like rebase, are enabled by default in Sapling.

#### Sapling has no “named branches”.
In Mercurial, a user may create bookmarks or branches.

In Sapling, there are only bookmarks.  “Named Branches” in the Mercurial sense do not exist.
#### Sapling has remote bookmarks.
In Mercurial, there are only local bookmarks which are synchronized with the server during push and pull.

In Sapling, there are local bookmarks which only ever exist locally, and there are remote bookmarks, such as remote/main, which are immutable local representations of the location of the server bookmark at the time of the last `sl pull`.
#### Sapling allows hiding/unhiding commits.
In Mercurial, to remove a commit you must either strip the commit entirely, or use an extension like “Evolve” to semi-permanently prune the commit.

In Sapling, commits are never removed/stripped from your repository and can easily be hidden/unhidden at will.
#### Sapling makes editing commits a first-class operation.
The original Mercurial design avoided editing commits.  While later extensions added some ability to edit commits (rebase, amend, strip, etc), it can still feel like a second-class feature.

Sapling treats editing commits as a first-class concept and provides a variety of commands for manipulating commits and recovering from manipulation mistakes.

# Similarities to Mercurial

#### Sapling supports the same revset and template features as Mercurial.
Revsets and templates largely work the same as they do in Mercurial.

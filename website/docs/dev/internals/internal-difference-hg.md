# Internal differences from Mercurial

:::note
This page assumes that you are familiar with Mercurial internals.
:::


## Visibility

Mercurial treats all commits as visible by default, using obsolescence data to
mark obsoleted commits as invisible.

Sapling treats all commits as invisible by default, using ["visible
heads"](/docs/dev/internals/visibility-and-mutation.md#commit-visibility)
and bookmark references to mark commits and their ancestors as visible. This
is similar to Git.

Performance wise, too much obsolescence data can slow down a Mercurial repo.
Similarly, too many bookmarks and visible heads can slow down a Sapling repo.
However, obsolescence data can grow over time unbounded while bookmarks and
visible heads can shrink using commands like `sl bookmark -d` and `sl hide`.
Practically, we assume a bounded number of bookmarks and visible heads.

Mercurial has a "repo view" layer to forbid access to hidden commits.
Accessing them (for example, using the `predecessors()` revset) requires a
global flag `--hidden`. Sapling removes the "repo view" layer. Revsets like
`all()`, `children()`, `descendants()` handle the visibility transparently by
not including invisible commits. Revsets like `predecessors()` do not care
about visibility and return invisible commits.  If the user explicitly requests
them using commit hashes, they will be included.


## Phase

Mercurial tracks phases (public, draft, secret) explicitly using "phase roots".
Commits are public by default. Draft and secret roots are explicitly listed.
The "phase roots" can grow unbounded and slow down the repo over time.

Sapling infers phases from remote bookmarks and visibility. Commits are secret
(invisible) by default. Main remote bookmarks and their ancestors are marked
public. Other visible commits are draft.

In Mercurial visibility and phase are separate concepts. A secret commit can be
visible or invisible. In Sapling "secret" is just an alias to "invisible" -
there are no "visible secret" commits.

## Obsolescence

Mercurial uses the "obsstore" to track commit rewrites. Sapling uses
["mutation"](/docs/dev/internals/visibility-and-mutation.md#commit-mutation). Their differences are:
- Obsstore decides visibility. Mutation does not decide visibility.
- Obsstore supports "prune" operation to remove a commit without a successor
  commit. Mutation requires at least one successor commit so it cannot track
  "prune" rewrites.
- If all successors of a mutation are invisible, then the mutation is ignored.
  This means mutation can be implicitly tracked by visibility. Restoring
  visibility to a previous state during an undo operation effectively
  restores the commit rewrite state.

Implementation wise, mutation uses IndexedLog for `O(log N)` lookup. Nothing in
Sapling requires `O(N)` loading of the entire mutation data.


## Storage format

Mercurial uses [Revlog](https://www.mercurial-scm.org/wiki/Revlog) as its main
file format. Sapling uses [IndexedLog](/docs/dev/internals/indexedlog.md) instead.

For working copy state, Mercurial uses [Dirstate](https://www.mercurial-scm.org/wiki/DirState).
Sapling switched to TreeState in 2017. Mercurial 5.9 released in 2021
introduced [Dirstate v2](https://www.mercurial-scm.org/repo/hg/file/tip/mercurial/helptext/internals/dirstate-v2.txt)
that improves performance in a similar way.

For repo references such as bookmarks and remote bookmarks, Mercurial tracks
them in individual files like `.hg/bookmarks`. Sapling uses [MetaLog](/docs/dev/internals/metalog.md)
to track them so changes across state files are atomic.


## Protocols

Mercurial supports ssh and http wireprotocols. Sapling's main protocol is
defined in a Rust `EdenApi` trait. It is very different from the original
wireprotocols.

There are two implementations of the `EdenApi` trait: an HTTP client that talks
to a supported server and an `EagerRepo` for lightweight local testing. The
HTTP implementation uses multiple connections to saturate network bandwidth
for better performance.


## Python 3 and Unicode

Python 3 switched the `str` type from `bytes` to `unicode`. This affects
keyword arguments, and stdlib APIs like `os.listdir`, `sys.argv`.

Sapling adopts Unicode more aggressively. Command line arguments, bookmark
names, file names, config files are considered Unicode and are encoded using
utf-8 during serialization. Sapling does not turn Python keyword arguments and
stdlib output back to bytes.

Treating file names as utf-8 allows Sapling to read and write correct file
names between Windows and \*nix systems for a given repo.


## Pure Python support

Mercurial maintains a pure Python implementation. It can run without building
with a C or Rust compiler by setting `HGMODULEPOLICY` to `py`. This is not
possible for Sapling.

## Ignore files

Mercurial supports `.hgignore`, optionally `.gitignore` through extensions.
Sapling only supports `.gitignore`.

## Git support

There are 2 extensions that add Git support to Mercurial:
- [hg-git](https://www.mercurial-scm.org/wiki/HgGit)
- [hgext/git](https://www.mercurial-scm.org/repo/hg/file/tip/hgext/git/__init__.py)


`hg-git` mirrors the bare Git repo to a regular hg repo. Therefore
it double stores file content, and produces different hashes.

`hgext/git` tries to be compatible with an existing Git repo. Therefore
it is limited to git specifications like what the `.git` directory should
contain and in what format.

Sapling treats Git as an implementation of its repo data abstraction.
This means:
- The working copy implementation is Sapling's. It can integrate with our
  virtualized working copy filesystem in the future.
- The repo data implementation can adopt Sapling's components in the future for
  benefits like on-demand fetching, data bookkeeping without repack.
- Git commands are not supported in Sapling's Git repo.

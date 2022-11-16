# MetaLog

MetaLog is used to track lightweight repo metadata like bookmarks, remote
bookmarks, visible heads, etc. It makes atomic updates possible, and allows
viewing past states for debugging and undoing.

## Background

Historically, repo metadata like bookmarks, remote bookmarks, and phases are
stored in separate files. Because reading is designed to be lock-free, and the
filesystem is not transactional, updating these files is not atomic and readers
might see inconsistent state where some files are updated but others aren't.
This requires careful design of file write order to reduce issues. The write
order can be subtle and fragile to maintain.

The other motivation is to help debug user issues. Sometimes it's really
helpful to understand what happened in the past. Metalog tracks this historical
data to answer questions like how and when bookmarks changed, etc.

## MetaLog

### Structure

MetaLog maintains 2 structures:
- A blob store backed by [ZStore](zstdelta#zstore). Blobs are keyed by their
  content SHA1s. There are 2 kinds of blobs: root, and content.
- A log of keys of roots. It provides a way to get the latest root, and also
  historical roots.

A root blob contains:
- A description of why this root was created.
- A map from (file) names to keys of content blobs.
- Keys of parent roots.

You might notice that MetaLog is kind of like a lightweight source control system
itself. That is part of the reason for the naming.

### Concurrent writes

Similar to [IndexedLog](indexedlog#concurrent-writes), changes are buffered in
memory until an explicit flush to disk.

Unlike IndexedLog, if MetaLog notices that the latest root is changed on disk,
it will attempt to perform a merge defined using a merge function specified by
the application. A merge failure will prevent MetaLog from committing the
changes to disk.

This means the application might not need extra locking, instead relying on
MetaLog's merge feature to detect races.

## Usage in Sapling

### Integration with transaction

In Sapling, code like:

```python
with repo.lock(), repo.transaction("tranaction-name") as tr:
    ...
```

Reloads the latest MetaLog root at the beginning of the transaction, and writes
changes back to disk at end of the transaction.

### Source of truth

To avoid invalidation issues, if performance allows, avoid storing states from
the MetaLog:

```python
class Repo:
    def __init__(self):
        self._foo = None

    def foo(self):
        # Fragile: Requries extra effort to ensure repo._foo is always synced
        # with source of truth.
        if self._foo is None:
            self._foo = decode_foo(self.metalog()['foo'])
        return self._foo

    def foo(self):
        # Less fragile: foo() is always synced with metalog source of truth.
        return decode_foo(self.metalog()['foo'])
```

### Other storage data

MetaLog is only intended to store lightweight metadata that deltas very well.

There are other kinds of data that are not so lightweight. For example, files,
trees, commits, and mutation records.

Sapling's strategy to maintain consistency is to ensure orphaned data in the
heavywight storage won't visibly affect the user experience. For example,
- Files or Trees: If there are unused files or trees stored, they do not
  affect the output of `sl` or `status`, etc. They are simply not referred to.
- Commits: Similarly, if there are extra commits stored in the commit graph,
  they are invisible because they are not referred to by visible heads or
  bookmarks. The orphaned commits are transparent to common commands.
- Mutation Records: If there are unused mutation records, since the successors
  are invisible, the records are simply ignored. However, it will turn a commit
  from `o` to `x` in `log -G` output.

This means that Sapling can just flush these kinds of data without going
through MetaLog, and there is no need to undo or truncate them to a previous
state.

#### Write order

Write MetaLog last. MetaLog tracks references to other data.

Different kinds of data have dependencies. This requires a write order.
Commits refer to trees. Trees refer to files. Bookmarks in MetaLog refer to
commits.

If MetaLog is written before writing commits, it might refer to unknown
commits and cause issues.

Whether commits or trees are written first does not matter, since there are no
references to them and they are just unused data described above.

### Export to Git

You can export MetaLog content to a Git repo:

    sl debugexportmetalog /path/to/git-repo

From there you can run commands like `git annotate remotenames` and
`git log -p remotenames` to see what commands changed a specific remote bookmark
and when.

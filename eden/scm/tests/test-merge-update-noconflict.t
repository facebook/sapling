#chg-compatible

  $ enable amend rebase
  $ setconfig experimental.updatecheck=noconflict

Updating w/ noconflict prints the conflicting changes:
  $ newrepo
  $ hg debugdrawdag <<'EOS'
  > c            # c/b = foo
  > |            # c/a = bar
  > b            # c/z = foo
  > |            # c/y = bar
  > |            # b/z = base
  > |            # b/y = base
  > a
  > EOS
  $ hg up b
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "conflict" | tee a b y z
  conflict
  $ hg up c
  abort: 4 conflicting file changes:
   a
   b
   y
   z
  (commit, shelve, update --clean to discard them, or update --merge to merge them)
  [255]

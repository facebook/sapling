#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig experimental.nativecheckout=true
  $ setconfig commands.update.check=noconflict
  $ newserver server

  $ newremoterepo myrepo

  $ echo a > a
  $ hg add a
  $ hg commit -m 'A'
  $ echo a > b
  $ hg add b
  $ hg commit -m 'B'
  $ hg up 'desc(A)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo x > b
  $ hg up 'desc(B)'
  b: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ hg up 'desc(B)' --clean
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up 'desc(A)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo a > b
  $ hg up 'desc(B)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm b
  $ hg rm b
  $ echo X > B
  $ hg add B
  warning: possible case-folding collision for B
  $ hg commit -m 'C'
  $ hg up 'desc(B)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ ls
  a
  b
  $ echo Z > a
  $ hg up 'desc(C)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg status
  M a
  $ hg up null
  abort: 1 conflicting file changes:
   a
  (commit, shelve, goto --clean to discard all your changes, or update --merge to merge them)
  [255]
#if no-windows
Replacing symlink with content
  $ mkdir x
  $ echo zzz > x/a
  $ ln -s x y
  $ hg add x/a y
  $ hg commit -m 'D'
  $ rm y
  $ hg rm y
  $ mkdir y
  $ echo yyy > y/a
  $ hg add y/a
  $ hg commit -m 'E'
  $ hg up 'desc(D)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat y/a
  zzz
  $ hg up 'desc(E)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat y/a
  yyy
#endif

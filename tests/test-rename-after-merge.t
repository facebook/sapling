Issue746: renaming files brought by the second parent of a merge was
broken.

Create source repository:

  $ hg init t
  $ cd t
  $ echo a > a
  $ hg ci -Am a
  adding a
  $ cd ..

Fork source repository:

  $ hg clone t t2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd t2
  $ echo b > b
  $ hg ci -Am b
  adding b

Update source repository:

  $ cd ../t
  $ echo a >> a
  $ hg ci -m a2

Merge repositories:

  $ hg pull ../t2
  pulling from ../t2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg st
  M b

Rename b as c:

  $ hg mv b c
  $ hg st
  A c
  R b

Rename back c as b:

  $ hg mv c b
  $ hg st
  M b

  $ cd ..

Issue 1476: renaming a first parent file into another first parent
file while none of them belong to the second parent was broken

  $ hg init repo1476
  $ cd repo1476
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ echo b1 > b1
  $ echo b2 > b2
  $ hg ci -Am changea
  adding b1
  adding b2
  $ hg up -C 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo c1 > c1
  $ echo c2 > c2
  $ hg ci -Am addcandd
  adding c1
  adding c2
  created new head

Merge heads:

  $ hg merge
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg mv -Af c1 c2

Commit issue 1476:

  $ hg ci -m merge

  $ hg log -r tip -C -v | grep copies
  copies:      c2 (c1)

  $ hg rollback
  repository tip rolled back to revision 2 (undo commit)
  working directory now based on revisions 2 and 1

  $ hg up -C .
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved

Merge heads again:

  $ hg merge
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg mv -Af b1 b2

Commit issue 1476 with a rename on the other side:

  $ hg ci -m merge

  $ hg log -r tip -C -v | grep copies
  copies:      b2 (b1)

  $ cd ..

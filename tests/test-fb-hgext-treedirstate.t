Setup

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > treedirstate=
  > [treedirstate]
  > useinnewrepos=True
  > EOF

  $ hg init repo
  $ cd repo
  $ echo base > base
  $ hg add base
  $ hg debugdirstate
  a   0         -1 unset               base
  $ hg commit -m "base"
  $ hg debugdirstate
  n 644          5 * base (glob)

Create path-conflicting dirstates

  $ hg up -q 0
  $ echo a > a
  $ hg add a
  $ hg commit -m a
  $ hg bookmark a
  $ hg up -q 0
  $ mkdir a
  $ echo a/a > a/a
  $ hg add a/a
  $ hg commit -m a/a
  $ hg bookmark a/a
  $ hg up -q a
  $ hg status
  $ hg rm a
  $ hg status
  R a
  $ hg merge --force a/a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg status
  M a/a
  R a
  $ hg rm --force a/a
  $ hg status
  R a
  R a/a
  $ hg up -Cq 0

Attempt to create a path conflict in the manifest

  $ echo b > b
  $ hg add b
  $ hg commit -m b
  $ rm b
  $ mkdir b
  $ echo b/b > b/b
  $ hg add b/b
  abort: file 'b' in dirstate clashes with 'b/b'
  [255]
  $ rm -rf b
  $ hg up -Cq 0

Test warning when creating files that might give a casefold collision

#if no-icasefs
  $ echo data > FiLeNaMe
  $ hg add FiLeNaMe
  $ echo data > FILENAME
  $ hg add FILENAME
  warning: possible case-folding collision for FILENAME
  $ rm -f FiLeNaMe FILENAME
  $ hg up -Cq 0
#endif

Test dirfoldmap and filefoldmap on case insensitive filesystems

#if icasefs
  $ mkdir -p dirA/dirB/dirC
  $ echo file1 > dira/File1
  $ echo file2 > dira/dirb/FILE2
  $ echo file3 > dira/dirb/dirc/FiLe3
  $ echo file4 > dira/dirb/dirc/file4
  $ hg add DIRA
  adding dirA/File1
  adding dirA/dirB/FILE2
  adding dirA/dirB/dirC/FiLe3
  adding dirA/dirB/dirC/file4
  $ hg status
  A dirA/File1
  A dirA/dirB/FILE2
  A dirA/dirB/dirC/FiLe3
  A dirA/dirB/dirC/file4
  $ hg forget dira/DIRB
  removing dirA/dirB/FILE2
  removing dirA/dirB/dirC/FiLe3
  removing dirA/dirB/dirC/file4
  $ hg status
  A dirA/File1
  ? dirA/dirB/FILE2
  ? dirA/dirB/dirC/FiLe3
  ? dirA/dirB/dirC/file4
  $ hg add dira/dirb/file2
  $ hg status
  A dirA/File1
  A dirA/dirB/FILE2
  ? dirA/dirB/dirC/FiLe3
  ? dirA/dirB/dirC/file4
  $ rm -rf dirA
#endif

Test autorepack

  $ ls .hg/dirstate.tree.*
  .hg/dirstate.tree.* (glob)
  $ echo data > file

After the first repack, the old trees are kept around by the transaction undo backups.
  $ hg add file --config treedirstate.minrepackthreshold=1 --config treedirstate.repackfactor=0 --debug | grep -v 'in use by'
  adding file
  auto-repacking treedirstate

After the second repack, the tree is replaced by a new tree and then deleted.
  $ hg forget file --config treedirstate.minrepackthreshold=1 --config treedirstate.repackfactor=0 --debug | grep -v 'in use by'
  removing file
  auto-repacking treedirstate
  $ ls .hg/dirstate.tree.*
  .hg/dirstate.tree.* (glob)
  .hg/dirstate.tree.* (glob)

Test downgrade on pull

  $ for f in 1 2 3 4 5 ; do mkdir dir$f ; echo $f > dir$f/file$f ; hg add dir$f/file$f ; done
  $ echo x > a
  $ hg add a
  $ hg commit -m "add files"
  $ cd ..
  $ hg clone repo clone
  updating to branch default
  7 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd clone
  $ hg merge 1
  merging a
  warning: conflicts while merging a! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ echo data > newfile
  $ hg add newfile
  $ hg rm dir3/file3
  $ grep treedirstate .hg/requires
  treedirstate
  $ hg pull --config treedirstate.downgradeonpull=true
  disabling treedirstate...
  pulling from $TESTTMP/repo (glob)
  searching for changes
  no changes found
  $ hg debugdirstate
  m   0         -2 * a (glob)
  n 644          5 * base (glob)
  n 644          2 * dir1/file1 (glob)
  n 644          2 * dir2/file2 (glob)
  r   0          0 * dir3/file3 (glob)
  n 644          2 * dir4/file4 (glob)
  n 644          2 * dir5/file5 (glob)
  a   0         -1 * newfile (glob)
  $ grep treedirstate .hg/requires
  [1]

Test upgrade on pull with conflicting dirstate reimplementation

  $ cat > $TESTTMP/fakesqldirstate.py << EOF
  > from mercurial import localrepo
  > def featuresetup(ui, supported):
  >     supported |= {'sqldirstate'}
  > def extsetup(ui):
  >     localrepo.localrepository.featuresetupfuncs.add(featuresetup)
  > EOF
  $ cp .hg/requires .hg/requires.test-backup
  $ echo sqldirstate >> .hg/requires
  $ hg pull --config treedirstate.upgradeonpull=true --config extensions.fakesqldirstate=$TESTTMP/fakesqldirstate.py
  pulling from $TESTTMP/repo (glob)
  searching for changes
  no changes found
  $ hg debugtreedirstate on --config extensions.fakesqldirstate=$TESTTMP/fakesqldirstate.py
  abort: repo has alternative dirstate active: sqldirstate
  [255]
  $ mv -f .hg/requires.test-backup .hg/requires

Test upgrade on pull

  $ hg pull --config treedirstate.upgradeonpull=true
  please wait while we migrate your repo to treedirstate
  this will make your hg commands faster...
  pulling from $TESTTMP/repo (glob)
  searching for changes
  no changes found
  $ hg debugdirstate
  m   0         -2 * a (glob)
  n 644          5 * base (glob)
  n 644          2 * dir1/file1 (glob)
  n 644          2 * dir2/file2 (glob)
  r   0          0 * dir3/file3 (glob)
  n 644          2 * dir4/file4 (glob)
  n 644          2 * dir5/file5 (glob)
  a   0         -1 * newfile (glob)
  $ grep treedirstate .hg/requires
  treedirstate
  $ hg pull --config treedirstate.upgradeonpull=true
  pulling from $TESTTMP/repo (glob)
  searching for changes
  no changes found

Test cleaning up on normal commands

  $ echo fakedirstatetree > .hg/dirstate.tree.fake
  $ hg forget newfile --debug --config treedirstate.cleanuppercent=100
  removing newfile
  $ test -f .hg/dirstate.tree.fake
  [1]

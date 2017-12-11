  $ echo '[extensions]' >> $HGRCPATH
  $ echo 'hgext.mq =' >> $HGRCPATH

  $ hg init repo
  $ cd repo

  $ echo foo > foo
  $ hg ci -qAm 'add a file'

  $ hg qinit

  $ hg qnew foo
  $ echo foo >> foo
  $ hg qrefresh -m 'append foo'

  $ hg qnew bar
  $ echo bar >> foo
  $ hg qrefresh -m 'append bar'

Try to operate on public mq changeset

  $ hg qpop
  popping bar
  now at: foo
  $ hg phase --public qbase
  $ echo babar >> foo
  $ hg qref
  abort: cannot qrefresh public revision
  (see 'hg help phases' for details)
  [255]
  $ hg revert -a
  reverting foo
  $ hg qpop
  abort: popping would remove a public revision
  (see 'hg help phases' for details)
  [255]
  $ hg qfold bar
  abort: cannot qrefresh public revision
  (see 'hg help phases' for details)
  [255]
  $ hg revert -a
  reverting foo

restore state for remaining test

  $ hg qpush
  applying bar
  now at: bar

try to commit on top of a patch

  $ echo quux >> foo
  $ hg ci -m 'append quux'
  abort: cannot commit over an applied mq patch
  [255]


cheat a bit...

  $ mv .hg/patches .hg/patches2
  $ hg ci -m 'append quux'
  $ mv .hg/patches2 .hg/patches


qpop/qrefresh on the wrong revision

  $ hg qpop
  abort: popping would remove a revision not managed by this patch queue
  [255]
  $ hg qpop -n patches
  using patch queue: $TESTTMP/repo/.hg/patches
  abort: popping would remove a revision not managed by this patch queue
  [255]
  $ hg qrefresh
  abort: working directory revision is not qtip
  [255]

  $ hg up -C qtip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg qpop
  abort: popping would remove a revision not managed by this patch queue
  [255]
  $ hg qrefresh
  abort: cannot qrefresh a revision with children
  [255]
  $ hg tip --template '{rev} {desc}\n'
  3 append quux


qpush warning branchheads

  $ cd ..
  $ hg init branchy
  $ cd branchy
  $ echo q > q
  $ hg add q
  $ hg qnew -f qp
  $ hg qpop
  popping qp
  patch queue now empty
  $ echo a > a
  $ hg ci -Ama
  adding a
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg branch b
  marked working directory as branch b
  (branches are permanent and global, did you want a bookmark?)
  $ echo c > c
  $ hg ci -Amc
  adding c
  $ hg merge default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -mmerge
  $ hg up default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log
  changeset:   2:65309210bf4e
  branch:      b
  tag:         tip
  parent:      1:707adb4c8ae1
  parent:      0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge
  
  changeset:   1:707adb4c8ae1
  branch:      b
  parent:      -1:000000000000
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
  changeset:   0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ hg qpush
  applying qp
  now at: qp

Testing applied patches, push and --force

  $ cd ..
  $ hg init forcepush
  $ cd forcepush
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ echo a >> a
  $ hg ci -m changea
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch branch
  marked working directory as branch branch
  (branches are permanent and global, did you want a bookmark?)
  $ echo b > b
  $ hg ci -Am addb
  adding b
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg --cwd .. clone -r 0 forcepush forcepush2
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 07f494440405
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo a >> a
  $ hg qnew patch

Pushing applied patch with --rev without --force

  $ hg push -r . ../forcepush2
  pushing to ../forcepush2
  abort: source has mq patches applied
  [255]

Pushing applied patch with branchhash, without --force

  $ hg push ../forcepush2#default
  pushing to ../forcepush2
  abort: source has mq patches applied
  [255]

Pushing revs excluding applied patch

  $ hg push --new-branch -r 'branch(branch)' -r 2 ../forcepush2
  pushing to ../forcepush2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

Pushing applied patch with --force

  $ hg phase --force --secret 'mq()'
  $ hg push --force -r default ../forcepush2
  pushing to ../forcepush2
  searching for changes
  no changes found (ignored 1 secret changesets)
  [1]
  $ hg phase --draft 'mq()'
  $ hg push --force -r default ../forcepush2
  pushing to ../forcepush2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)

  $ cd ..

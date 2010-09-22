  $ echo '[extensions]' >> $HGRCPATH
  $ echo 'mq =' >> $HGRCPATH

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
  using patch queue: */repo/.hg/patches (glob)
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
  abort: cannot refresh a revision with children
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

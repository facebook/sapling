  $ hg init

should fail

  $ hg add .hg/00changelog.i
  abort: path contains illegal component: .hg/00changelog.i
  [255]

  $ mkdir a
  $ echo a > a/a
  $ hg ci -Ama
  adding a/a
  $ ln -s a b
  $ echo b > a/b

should fail

  $ hg add b/b
  abort: path 'b/b' traverses symbolic link 'b'
  [255]

should succeed

  $ hg add b

should still fail - maybe

  $ hg add b/b
  abort: path 'b/b' traverses symbolic link 'b'
  [255]

unbundle tampered bundle

  $ hg init target
  $ cd target
  $ hg unbundle $TESTDIR/tampered.hg
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 6 changes to 6 files (+4 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

attack .hg/test

  $ hg manifest -r0
  .hg/test
  $ hg update -Cr0
  abort: path contains illegal component: .hg/test
  [255]

attack foo/.hg/test

  $ hg manifest -r1
  foo/.hg/test
  $ hg update -Cr1
  abort: path 'foo/.hg/test' is inside repo 'foo'
  [255]

attack back/test where back symlinks to ..

  $ hg manifest -r2
  back
  back/test
  $ hg update -Cr2
  abort: path 'back/test' traverses symbolic link 'back'
  [255]

attack ../test

  $ hg manifest -r3
  ../test
  $ hg update -Cr3
  abort: path contains illegal component: ../test
  [255]

attack /tmp/test

  $ hg manifest -r4
  /tmp/test
  $ hg update -Cr4
  abort: No such file or directory: $TESTTMP/target//tmp/test
  [255]

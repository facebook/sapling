  $ hg init

audit of .hg

  $ hg add .hg/00changelog.i
  abort: path contains illegal component: .hg/00changelog.i (glob)
  [255]

#if symlink

Symlinks

  $ mkdir a
  $ echo a > a/a
  $ hg ci -Ama
  adding a/a
  $ ln -s a b
  $ echo b > a/b
  $ hg add b/b
  abort: path 'b/b' traverses symbolic link 'b' (glob)
  [255]
  $ hg add b

should still fail - maybe

  $ hg add b/b
  abort: path 'b/b' traverses symbolic link 'b' (glob)
  [255]

  $ hg commit -m 'add symlink b'


Test symlink traversing when accessing history:
-----------------------------------------------

(build a changeset where the path exists as a directory)

  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkdir b
  $ echo c > b/a
  $ hg add b/a
  $ hg ci -m 'add directory b'
  created new head

Test that hg cat does not do anything wrong the working copy has 'b' as directory

  $ hg cat b/a
  c
  $ hg cat -r "desc(directory)" b/a
  c
  $ hg cat -r "desc(symlink)" b/a
  b/a: no such file in rev bc151a1f53bd
  [1]

Test that hg cat does not do anything wrong the working copy has 'b' as a symlink (issue4749)

  $ hg up 'desc(symlink)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg cat b/a
  b/a: no such file in rev bc151a1f53bd
  [1]
  $ hg cat -r "desc(directory)" b/a
  c
  $ hg cat -r "desc(symlink)" b/a
  b/a: no such file in rev bc151a1f53bd
  [1]

#endif


unbundle tampered bundle

  $ hg init target
  $ cd target
  $ hg unbundle "$TESTDIR/bundles/tampered.hg"
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 6 changes to 6 files (+4 heads)
  new changesets b7da9bf6b037:fc1393d727bc
  (run 'hg heads' to see heads, 'hg merge' to merge)

attack .hg/test

  $ hg manifest -r0
  .hg/test
  $ hg update -Cr0
  abort: path contains illegal component: .hg/test (glob)
  [255]

attack foo/.hg/test

  $ hg manifest -r1
  foo/.hg/test
  $ hg update -Cr1
  abort: path 'foo/.hg/test' is inside nested repo 'foo' (glob)
  [255]

attack back/test where back symlinks to ..

  $ hg manifest -r2
  back
  back/test
#if symlink
  $ hg update -Cr2
  abort: path 'back/test' traverses symbolic link 'back'
  [255]
#else
('back' will be a file and cause some other system specific error)
  $ hg update -Cr2
  back: is both a file and a directory
  abort: * (glob)
  [255]
#endif

attack ../test

  $ hg manifest -r3
  ../test
  $ mkdir ../test
  $ echo data > ../test/file
  $ hg update -Cr3
  abort: path contains illegal component: ../test (glob)
  [255]
  $ cat ../test/file
  data

attack /tmp/test

  $ hg manifest -r4
  /tmp/test
  $ hg update -Cr4
  abort: path contains illegal component: /tmp/test (glob)
  [255]

  $ cd ..

Test symlink traversal on merge:
--------------------------------

#if symlink

set up symlink hell

  $ mkdir merge-symlink-out
  $ hg init merge-symlink
  $ cd merge-symlink
  $ touch base
  $ hg commit -qAm base
  $ ln -s ../merge-symlink-out a
  $ hg commit -qAm 'symlink a -> ../merge-symlink-out'
  $ hg up -q 0
  $ mkdir a
  $ touch a/poisoned
  $ hg commit -qAm 'file a/poisoned'
  $ hg log -G -T '{rev}: {desc}\n'
  @  2: file a/poisoned
  |
  | o  1: symlink a -> ../merge-symlink-out
  |/
  o  0: base
  

try trivial merge

  $ hg up -qC 1
  $ hg merge 2
  abort: path 'a/poisoned' traverses symbolic link 'a'
  [255]

try rebase onto other revision: cache of audited paths should be discarded,
and the rebase should fail (issue5628)

  $ hg up -qC 2
  $ hg rebase -s 2 -d 1 --config extensions.rebase=
  rebasing 2:e73c21d6b244 "file a/poisoned" (tip)
  abort: path 'a/poisoned' traverses symbolic link 'a'
  [255]
  $ ls ../merge-symlink-out

  $ cd ..

Test symlink traversal on update:
---------------------------------

  $ mkdir update-symlink-out
  $ hg init update-symlink
  $ cd update-symlink
  $ ln -s ../update-symlink-out a
  $ hg commit -qAm 'symlink a -> ../update-symlink-out'
  $ hg rm a
  $ mkdir a && touch a/b
  $ hg ci -qAm 'file a/b' a/b
  $ hg up -qC 0
  $ hg rm a
  $ mkdir a && touch a/c
  $ hg ci -qAm 'rm a, file a/c'
  $ hg log -G -T '{rev}: {desc}\n'
  @  2: rm a, file a/c
  |
  | o  1: file a/b
  |/
  o  0: symlink a -> ../update-symlink-out
  

try linear update where symlink already exists:

  $ hg up -qC 0
  $ hg up 1
  abort: path 'a/b' traverses symbolic link 'a'
  [255]

try linear update including symlinked directory and its content: paths are
audited first by calculateupdates(), where no symlink is created so both
'a' and 'a/b' are taken as good paths. still applyupdates() should fail.

  $ hg up -qC null
  $ hg up 1
  abort: path 'a/b' traverses symbolic link 'a'
  [255]
  $ ls ../update-symlink-out

try branch update replacing directory with symlink, and its content: the
path 'a' is audited as a directory first, which should be audited again as
a symlink.

  $ rm -f a
  $ hg up -qC 2
  $ hg up 1
  abort: path 'a/b' traverses symbolic link 'a'
  [255]
  $ ls ../update-symlink-out

  $ cd ..

#endif

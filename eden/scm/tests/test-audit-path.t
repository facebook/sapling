#chg-compatible
#debugruntest-compatible

  $ setconfig workingcopy.ruststatus=False
  $ disable treemanifest
  $ newrepo

audit of .hg

  $ hg add .hg/00changelog.i
  abort: path contains illegal component: .hg/00changelog.i
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
  abort: path 'b/b' traverses symbolic link 'b'
  [255]
  $ hg add b

should still fail - maybe

  $ hg add b/b
  abort: path 'b/b' traverses symbolic link 'b'
  [255]

  $ hg commit -m 'add symlink b'


Test symlink traversing when accessing history:
-----------------------------------------------

(build a changeset where the path exists as a directory)

  $ hg up 1111159f092d71b564a6977fa2b7efd3edb15847
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkdir b
  $ echo c > b/a
  $ hg add b/a
  $ hg ci -m 'add directory b'

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

  $ newrepo target
  $ hg unbundle "$TESTDIR/bundles/tampered.hg"
  adding changesets
  adding manifests
  adding file changes

attack .hg/test

  $ hg manifest -rb7da9bf6b037936363b456cdc950279bf7edb320
  .hg/test
  $ hg goto -Crb7da9bf6b037936363b456cdc950279bf7edb320
  abort: path contains illegal component: .hg/test
  [255]

attack foo/.hg/test

  $ hg manifest -r64cae21979bae51b18687c5777e4765dfa3397ab
  foo/.hg/test
  $ hg goto -Cr64cae21979bae51b18687c5777e4765dfa3397ab
  abort: path 'foo/.hg/test' is inside nested repo 'foo'
  [255]

attack back/test where back symlinks to ..

  $ hg manifest -r4d3561dc450daacc43cf09e0ff551cd94cff8662
  back
  back/test
#if symlink
  $ hg goto -Cr4d3561dc450daacc43cf09e0ff551cd94cff8662
  abort: path 'back/test' traverses symbolic link 'back'
  [255]
#else
('back' will be a file and cause some other system specific error)
  $ hg goto -Cr2
  back: is both a file and a directory
  abort: * (glob)
  [255]
#endif

attack ../test

  $ hg manifest -r40f5112af0dd4eea43a68a26af2433ee68c45ae6
  ../test
  $ mkdir ../test
  $ echo data > ../test/file
  $ hg goto -Cr40f5112af0dd4eea43a68a26af2433ee68c45ae6
  abort: path contains illegal component: ../test
  [255]
  $ cat ../test/file
  data

attack /tmp/test

  $ hg manifest -r'max(desc(add))'
  /tmp/test
  $ hg goto -Cr'max(desc(add))'
  abort: path contains illegal component: /tmp/test
  [255]

Test symlink traversal on merge:
--------------------------------

#if symlink

set up symlink hell

  $ cd "$TESTTMP"
  $ mkdir merge-symlink-out
  $ hg init merge-symlink
  $ cd merge-symlink
  $ touch base
  $ hg commit -qAm base
  $ ln -s ../merge-symlink-out a
  $ hg commit -qAm 'symlink a -> ../merge-symlink-out'
  $ hg up -q 'desc(base)'
  $ mkdir a
  $ touch a/poisoned
  $ hg commit -qAm 'file a/poisoned'
  $ hg log -G -T '{desc}\n'
  @  file a/poisoned
  │
  │ o  symlink a -> ../merge-symlink-out
  ├─╯
  o  base
  

try trivial merge

  $ hg up -qC 'desc(symlink)'
  $ hg merge 'desc(file)'
  abort: path 'a/poisoned' traverses symbolic link 'a'
  [255]

try rebase onto other revision: cache of audited paths should be discarded,
and the rebase should fail (issue5628)

  $ hg up -qC 'desc(file)'
  $ hg rebase -s 'desc(file)' -d 'desc(symlink)' --config extensions.rebase=
  rebasing e73c21d6b244 "file a/poisoned"
  abort: path 'a/poisoned' traverses symbolic link 'a'
  [255]
  $ ls ../merge-symlink-out

Test symlink traversal on update:
---------------------------------

  $ cd "$TESTTMP"
  $ mkdir update-symlink-out
  $ hg init update-symlink
  $ cd update-symlink
  $ ln -s ../update-symlink-out a
  $ hg commit -qAm 'symlink a -> ../update-symlink-out'
  $ hg rm a
  $ mkdir a && touch a/b
  $ hg ci -qAm 'file a/b' a/b
  $ hg up -qC 'desc(symlink)'
  $ hg rm a
  $ mkdir a && touch a/c
  $ hg ci -qAm 'rm a, file a/c'
  $ hg log -G -T '{desc}\n'
  @  rm a, file a/c
  │
  │ o  file a/b
  ├─╯
  o  symlink a -> ../update-symlink-out
  

try linear update where symlink already exists:

  $ hg up -qC 'desc(symlink)'
  $ hg up 82142393ba17c645380584deaedbc8cfe6eac24b
  abort: path 'a/b' traverses symbolic link 'a'
  [255]

try linear update including symlinked directory and its content: paths are
audited first by calculateupdates(), where no symlink is created so both
'a' and 'a/b' are taken as good paths. still applyupdates() should fail.

  $ hg up -qC null
  $ hg up 82142393ba17c645380584deaedbc8cfe6eac24b
  abort: path 'a/b' traverses symbolic link 'a'
  [255]
  $ ls ../update-symlink-out

try branch update replacing directory with symlink, and its content: the
path 'a' is audited as a directory first, which should be audited again as
a symlink.

  $ rm -f a
  $ hg up -qC 'desc(rm)'
  $ hg up 82142393ba17c645380584deaedbc8cfe6eac24b
  abort: path 'a/b' traverses symbolic link 'a'
  [255]
  $ ls ../update-symlink-out

#endif

Works for .sl repos also

  $ HGIDENTITY=sl newrepo
  $ hg add .sl/00changelog.i
  abort: path contains illegal component: .sl/00changelog.i
  [255]

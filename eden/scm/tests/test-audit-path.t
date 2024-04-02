#debugruntest-compatible

#require no-eden


  $ configure modernclient
  $ newclientrepo

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

  $ hg up .^
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
  [1]

Test that hg cat does not do anything wrong the working copy has 'b' as a symlink (issue4749)

  $ hg up 'desc(symlink)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg cat b/a
  [1]
  $ hg cat -r "desc(directory)" b/a
  c
  $ hg cat -r "desc(symlink)" b/a
  [1]

#endif

Test symlink traversal on merge:
--------------------------------

#if symlink

set up symlink hell

  $ cd "$TESTTMP"
  $ mkdir merge-symlink-out
  $ newclientrepo
  $ touch base
  $ hg commit -qAm base
  $ ln -s ../merge-symlink-out a
  $ hg commit -qAm 'symlink a -> ../merge-symlink-out'
  $ hg up -q 'desc(base)'
  $ mkdir a
  $ echo not-owned > a/poisoned
  $ hg commit -qAm 'file a/poisoned'
  $ hg log -G -T '{desc}\n'
  @  file a/poisoned
  │
  │ o  symlink a -> ../merge-symlink-out
  ├─╯
  o  base
  

try trivial merge

  $ hg up -qC 'desc(symlink)'
  $ hg merge -q 'desc(file)'
  $ hg st
  M a/poisoned
  ! a
  $ cat a/poisoned
  not-owned
  $ ls ../merge-symlink-out

try rebase onto other revision: cache of audited paths should be discarded,
and the rebase should fail (issue5628)

  $ hg up -qC 'desc(file)'
  $ hg rebase -q -s 'desc(file)' -d 'desc(symlink)' --config extensions.rebase=
  $ cat a/poisoned
  not-owned
  $ ls ../merge-symlink-out

Test symlink traversal on update:
---------------------------------

  $ cd "$TESTTMP"
  $ mkdir update-symlink-out
  $ newclientrepo
  $ ln -s ../update-symlink-out a
  $ hg commit -qAm 'symlink a -> ../update-symlink-out'
  $ hg rm a
  $ mkdir a && echo b > a/b
  $ hg ci -qAm 'file a/b' a/b
  $ hg up -qC 'desc(symlink)'
  $ hg rm a
  $ mkdir a && echo c > a/c
  $ hg ci -qAm 'rm a, file a/c'
  $ hg log -G -T '{desc}\n'
  @  rm a, file a/c
  │
  │ o  file a/b
  ├─╯
  o  symlink a -> ../update-symlink-out
  

try linear update where symlink already exists:

  $ hg up -qC 'desc(symlink)'
  $ hg up -q 'desc("file a/b")'
  $ cat a/b
  b

try linear update including symlinked directory and its content: paths are
audited first by calculateupdates(), where no symlink is created so both
'a' and 'a/b' are taken as good paths. still applyupdates() should fail.

  $ hg up -qC null
  $ hg up -q 'desc("file a/b")'
  $ cat a/b
  b
  $ ls ../update-symlink-out

try branch update replacing directory with symlink, and its content: the
path 'a' is audited as a directory first, which should be audited again as
a symlink.

  $ rm -f a
  $ hg up -qC 'desc(rm)'
  $ hg up -q 'desc("file a/b")'
  $ cat a/b
  b
  $ ls ../update-symlink-out

#endif

Works for .sl repos also

  $ HGIDENTITY=sl newrepo
  $ hg add .sl/00changelog.i
  abort: path contains illegal component: .sl/00changelog.i
  [255]

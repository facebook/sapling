#debugruntest-compatible
#require symlink

  $ eagerepo
  $ setconfig experimental.allowfilepeer=True

== tests added in 0.7 ==

  $ newclientrepo test-symlinks-0.7
  $ touch foo; ln -s foo bar; ln -s nonexistent baz

import with add and addremove -- symlink walking should _not_ screwup.

  $ hg add
  adding bar
  adding baz
  adding foo
  $ hg forget bar baz foo
  $ hg addremove
  adding bar
  adding baz
  adding foo

commit -- the symlink should _not_ appear added to dir state

  $ hg commit -m 'initial'

  $ touch bomb

again, symlink should _not_ show up on dir state

  $ hg addremove
  adding bomb

Assert screamed here before, should go by without consequence

  $ hg commit -m 'is there a bug?'
  $ cd ..

#if mkfifo

== fifo & ignore ==

  $ newclientrepo

  $ mkdir dir
  $ touch a.c dir/a.o dir/b.o

test what happens if we want to trick hg

  $ hg commit -A -m 0
  adding a.c
  adding dir/a.o
  adding dir/b.o
  $ echo "*.o" > .gitignore
  $ rm a.c
  $ rm dir/a.o
  $ rm dir/b.o
  $ mkdir dir/a.o
  $ ln -s nonexistent dir/b.o
  $ mkfifo a.c

it should show a.c, dir/a.o and dir/b.o deleted

  $ hg status
  a.c: invalid file type (no-fsmonitor !)
  M dir/b.o
  ! a.c
  ! dir/a.o
  ? .gitignore
  $ hg status a.c
  a.c: invalid file type
  ! a.c
  $ cd ..

#endif

== symlinks from outside the tree ==

test absolute path through symlink outside repo

  $ p=`pwd`
  $ hg init x
  $ ln -s x y
  $ cd x
  $ touch f
  $ hg add f
  $ hg status "$p"/y/f
  A f

try symlink outside repo to file inside

  $ mkdir foo bar
  $ echo a > foo/a
  $ ln -s `pwd`/foo/a bar/in-repo-symlink
  $ hg add -q
  $ hg st
  A bar/in-repo-symlink
  A f
  A foo/a
  $ ln -s `pwd`/bar ../bar

Show that we follow $TESTTMP/bar symlink into repo, but don't follow in-repo-symlink:

  $ hg revert ../bar/in-repo-symlink
  $ hg st
  A f
  A foo/a
  ? bar/in-repo-symlink

  $ cd ..

== cloning symlinks ==
  $ hg init clone; cd clone;

try cloning symlink in a subdir
1. commit a symlink

  $ mkdir -p a/b/c
  $ cd a/b/c
  $ ln -s /path/to/symlink/source demo
  $ cd ../../..
  $ hg stat
  ? a/b/c/demo
  $ hg add -q
  $ hg diff --git
  diff --git a/a/b/c/demo b/a/b/c/demo
  new file mode 120000
  --- /dev/null
  +++ b/a/b/c/demo
  @@ -0,0 +1,1 @@
  +/path/to/symlink/source
  \ No newline at end of file
  $ hg commit -m 'add symlink in a/b/c subdir'
  $ hg show --stat --git
  commit:      7c0e359fc055
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a/b/c/demo
  description:
  add symlink in a/b/c subdir
  
  
   a/b/c/demo |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)

2. clone it
  $ cd ..
  $ hg clone clone clonedest
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

== symlink and git diffs ==

test diff --git with symlinks

  $ cd clonedest
  $ hg diff --git -r null:tip
  diff --git a/a/b/c/demo b/a/b/c/demo
  new file mode 120000
  --- /dev/null
  +++ b/a/b/c/demo
  @@ -0,0 +1,1 @@
  +/path/to/symlink/source
  \ No newline at end of file
  $ hg export --git tip > ../sl.diff

import git-style symlink diff

  $ hg rm a/b/c/demo
  $ hg commit -m'remove link'
  $ hg import ../sl.diff
  applying ../sl.diff
  $ hg diff --git -r 'desc(remove)':tip
  diff --git a/a/b/c/demo b/a/b/c/demo
  new file mode 120000
  --- /dev/null
  +++ b/a/b/c/demo
  @@ -0,0 +1,1 @@
  +/path/to/symlink/source
  \ No newline at end of file

== symlinks and addremove ==

directory moved and symlinked

  $ mkdir foo
  $ touch foo/a

This avoids same-second race condition that leaves files as NEED_CHECK.
  $ sleep 1

  $ hg ci -Ama
  adding foo/a

#if fsmonitor
Make sure files are _not_ NEED_CHECK and have metadata. This is the tricky
case for "status" to detect the new symlink.
  $ hg debugtree list
  a/b/c/demo: 01207* 23 + EXIST_P1 EXIST_NEXT  (glob) (no-windows !)
  a/b/c/demo: 0120666 0 + EXIST_P1 EXIST_NEXT  (windows !)
  foo/a: 0100644 0 + EXIST_P1 EXIST_NEXT  (no-windows !)
  foo/a: 0100666 0 + EXIST_P1 EXIST_NEXT  (windows !)
#endif

  $ mv foo bar
  $ ln -s bar foo
  $ hg status
  ! foo/a
  ? bar/a
  ? foo

now addremove should remove old files

  $ hg addremove
  adding bar/a
  adding foo
  removing foo/a

commit and update back

  $ hg ci -mb
  $ hg up '.^'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg up tip
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cd ..

== root of repository is symlinked ==

  $ hg init root
  $ ln -s root link
  $ cd root
  $ echo foo > foo
  $ hg status
  ? foo
  $ hg status ../link
  ? foo
  $ hg add foo
  $ hg cp foo "$TESTTMP/link/bar"
  foo has not been committed yet, so no copy data will be stored for bar.
  $ cd ..


  $ newclientrepo
  $ ln -s nothing dangling
  $ hg commit -m 'commit symlink without adding' dangling
  abort: dangling: file not tracked!
  [255]
  $ hg add dangling
  $ hg commit -m 'add symlink'

  $ hg tip -v
  commit:      cabd88b706fc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       dangling
  description:
  add symlink


  $ hg manifest --debug
  2564acbe54bbbedfbf608479340b359f04597f80 644 @ dangling
  $ f dangling
  dangling -> nothing

  $ rm dangling
  $ ln -s void dangling
  $ hg commit -m 'change symlink'
  $ f dangling
  dangling -> void


modifying link

  $ rm dangling
  $ ln -s empty dangling
  $ f dangling
  dangling -> empty


reverting to rev 0:

  $ hg revert -r 'desc(add)' -a
  reverting dangling
  $ f dangling
  dangling -> nothing

  $ hg up -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

copies

  $ hg cp -v dangling dangling2
  copying dangling to dangling2
  $ hg st -Cmard
  A dangling2
    dangling
  $ f dangling dangling2
  dangling -> void
  dangling2 -> void


Issue995: hg copy -A incorrectly handles symbolic links

  $ hg up -C
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkdir dir
  $ ln -s dir dirlink
  $ hg ci -qAm 'add dirlink'
  $ mkdir newdir
  $ mv dir newdir/dir
  $ mv dirlink newdir/dirlink
  $ hg mv -A dirlink newdir/dirlink

  $ cd ..

Don't treat symlinks as untrackable if symlinks aren't supported.
  $ newclientrepo
  $ ln -s foo bar
  $ SL_DEBUG_DISABLE_SYMLINKS=1 hg status
  ? bar

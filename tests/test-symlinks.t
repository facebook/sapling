  $ "$TESTDIR/hghave" symlink || exit 80

== tests added in 0.7 ==

  $ hg init test-symlinks-0.7; cd test-symlinks-0.7;
  $ touch foo; ln -s foo bar;

import with addremove -- symlink walking should _not_ screwup.

  $ hg addremove
  adding bar
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


== fifo & ignore ==

  $ hg init test; cd test;

  $ mkdir dir
  $ touch a.c dir/a.o dir/b.o

test what happens if we want to trick hg

  $ hg commit -A -m 0
  adding a.c
  adding dir/a.o
  adding dir/b.o
  $ echo "relglob:*.o" > .hgignore
  $ rm a.c
  $ rm dir/a.o
  $ rm dir/b.o
  $ mkdir dir/a.o
  $ ln -s nonexist dir/b.o
  $ mkfifo a.c

it should show a.c, dir/a.o and dir/b.o deleted

  $ hg status
  M dir/b.o
  ! a.c
  ! dir/a.o
  ? .hgignore
  $ hg status a.c
  a.c: unsupported file type (type is fifo)
  ! a.c
  $ cd ..


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

  $ ln -s x/f ../z

this should fail

  $ hg status ../z && { echo hg mistakenly exited with status 0; exit 1; } || :
  abort: ../z not under root
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
  $ hg commit -A -m 'add symlink in a/b/c subdir'
  adding a/b/c/demo

2. clone it

  $ cd ..
  $ hg clone clone clonedest
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved


== symlink and git diffs ==

git symlink diff

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

import git symlink diff

  $ hg rm a/b/c/demo
  $ hg commit -m'remove link'
  $ hg import ../sl.diff
  applying ../sl.diff
  $ hg diff --git -r 1:tip
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
  $ hg ci -Ama
  adding foo/a
  $ mv foo bar
  $ ln -s bar foo

now addremove should remove old files

  $ hg addremove
  adding bar/a
  adding foo
  removing foo/a
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
  $ cd ..




  $ hg init b
  $ cd b
  $ ln -s nothing dangling
  $ hg commit -m 'commit symlink without adding' dangling
  abort: dangling: file not tracked!
  [255]
  $ hg add dangling
  $ hg commit -m 'add symlink'

  $ hg tip -v
  changeset:   0:cabd88b706fc
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       dangling
  description:
  add symlink
  
  
  $ hg manifest --debug
  2564acbe54bbbedfbf608479340b359f04597f80 644 @ dangling
  $ $TESTDIR/readlink.py dangling
  dangling -> nothing

  $ rm dangling
  $ ln -s void dangling
  $ hg commit -m 'change symlink'
  $ $TESTDIR/readlink.py dangling
  dangling -> void


modifying link

  $ rm dangling
  $ ln -s empty dangling
  $ $TESTDIR/readlink.py dangling
  dangling -> empty


reverting to rev 0:

  $ hg revert -r 0 -a
  reverting dangling
  $ $TESTDIR/readlink.py dangling
  dangling -> nothing


backups:

  $ $TESTDIR/readlink.py *.orig
  dangling.orig -> empty
  $ rm *.orig
  $ hg up -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

copies

  $ hg cp -v dangling dangling2
  copying dangling to dangling2
  $ hg st -Cmard
  A dangling2
    dangling
  $ $TESTDIR/readlink.py dangling dangling2
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


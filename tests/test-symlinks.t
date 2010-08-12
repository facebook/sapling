Test bug regarding symlinks that showed up in hg 0.7
Author: Matthew Elder <sseses@gmail.com>

  $ "$TESTDIR/hghave" symlink || exit 80

make and initialize repo

  $ hg init test; cd test;

make a file and a symlink

  $ touch foo; ln -s foo bar;

import with addremove -- symlink walking should _not_ screwup.

  $ hg addremove
  adding bar
  adding foo

commit -- the symlink should _not_ appear added to dir state

  $ hg commit -m 'initial'

add a new file so hg will let me commit again

  $ touch bomb

again, symlink should _not_ show up on dir state

  $ hg addremove
  adding bomb

Assert screamed here before, should go by without consequence

  $ hg commit -m 'is there a bug?'

  $ cd .. ; rm -r test
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

test absolute path through symlink outside repo

  $ cd ..
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

  $ cd .. ; rm -r test
  $ hg init test; cd test;

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
  $ hg clone test testclone
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

git symlink diff

  $ cd testclone
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

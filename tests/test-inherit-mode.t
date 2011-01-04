test that new files created in .hg inherit the permissions from .hg/store


  $ "$TESTDIR/hghave" unix-permissions || exit 80

  $ mkdir dir

just in case somebody has a strange $TMPDIR

  $ chmod g-s dir
  $ cd dir

  $ cat >printmodes.py <<EOF
  > import os, sys
  > 
  > allnames = []
  > isdir = {}
  > for root, dirs, files in os.walk(sys.argv[1]):
  >     for d in dirs:
  >         name = os.path.join(root, d)
  >         isdir[name] = 1
  >         allnames.append(name)
  >     for f in files:
  >         name = os.path.join(root, f)
  >         allnames.append(name)
  > allnames.sort()
  > for name in allnames:
  >     suffix = name in isdir and '/' or ''
  >     print '%05o %s%s' % (os.lstat(name).st_mode & 07777, name, suffix)
  > EOF

  $ cat >mode.py <<EOF
  > import sys
  > import os
  > print '%05o' % os.lstat(sys.argv[1]).st_mode
  > EOF

  $ umask 077

  $ hg init repo
  $ cd repo

  $ chmod 0770 .hg/store

before commit
store can be written by the group, other files cannot
store is setgid

  $ python ../printmodes.py .
  00700 ./.hg/
  00600 ./.hg/00changelog.i
  00600 ./.hg/requires
  00770 ./.hg/store/

  $ mkdir dir
  $ touch foo dir/bar
  $ hg ci -qAm 'add files'

after commit
working dir files can only be written by the owner
files created in .hg can be written by the group
(in particular, store/**, dirstate, branch cache file, undo files)
new directories are setgid

  $ python ../printmodes.py .
  00700 ./.hg/
  00600 ./.hg/00changelog.i
  00660 ./.hg/dirstate
  00660 ./.hg/last-message.txt
  00600 ./.hg/requires
  00770 ./.hg/store/
  00660 ./.hg/store/00changelog.i
  00660 ./.hg/store/00manifest.i
  00770 ./.hg/store/data/
  00770 ./.hg/store/data/dir/
  00660 ./.hg/store/data/dir/bar.i
  00660 ./.hg/store/data/foo.i
  00660 ./.hg/store/fncache
  00660 ./.hg/store/undo
  00660 ./.hg/undo.branch
  00660 ./.hg/undo.desc
  00660 ./.hg/undo.dirstate
  00700 ./dir/
  00600 ./dir/bar
  00600 ./foo

  $ umask 007
  $ hg init ../push

before push
group can write everything

  $ python ../printmodes.py ../push
  00770 ../push/.hg/
  00660 ../push/.hg/00changelog.i
  00660 ../push/.hg/requires
  00770 ../push/.hg/store/

  $ umask 077
  $ hg -q push ../push

after push
group can still write everything

  $ python ../printmodes.py ../push
  00770 ../push/.hg/
  00660 ../push/.hg/00changelog.i
  00770 ../push/.hg/cache/
  00660 ../push/.hg/cache/branchheads
  00660 ../push/.hg/requires
  00770 ../push/.hg/store/
  00660 ../push/.hg/store/00changelog.i
  00660 ../push/.hg/store/00manifest.i
  00770 ../push/.hg/store/data/
  00770 ../push/.hg/store/data/dir/
  00660 ../push/.hg/store/data/dir/bar.i
  00660 ../push/.hg/store/data/foo.i
  00660 ../push/.hg/store/fncache
  00660 ../push/.hg/store/undo
  00660 ../push/.hg/undo.branch
  00660 ../push/.hg/undo.desc
  00660 ../push/.hg/undo.dirstate


Test that we don't lose the setgid bit when we call chmod.
Not all systems support setgid directories (e.g. HFS+), so
just check that directories have the same mode.

  $ cd ..
  $ hg init setgid
  $ cd setgid
  $ chmod g+rwx .hg/store
  $ chmod g+s .hg/store 2> /dev/null
  $ mkdir dir
  $ touch dir/file
  $ hg ci -qAm 'add dir/file'
  $ storemode=`python ../mode.py .hg/store`
  $ dirmode=`python ../mode.py .hg/store/data/dir`
  $ if [ "$storemode" != "$dirmode" ]; then
  >  echo "$storemode != $dirmode"
  $ fi

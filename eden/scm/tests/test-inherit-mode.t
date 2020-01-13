#chg-compatible

  $ setconfig extensions.treemanifest=!
#require unix-permissions

test that new files created in .hg inherit the permissions from .hg/store

  $ mkdir dir

just in case somebody has a strange $TMPDIR

  $ chmod g-s dir
  $ cd dir

  $ cat >printmodes.py <<EOF
  > from __future__ import absolute_import, print_function
  > import os
  > import sys
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
  >     if 'blackbox' in name:
  >         continue
  >     suffix = name in isdir and '/' or ''
  >     print('%05o %s%s' % (os.lstat(name).st_mode & 0o7777, name, suffix))
  > EOF

  $ cat >mode.py <<EOF
  > from __future__ import absolute_import, print_function
  > import os
  > import sys
  > print('%05o' % os.lstat(sys.argv[1]).st_mode)
  > EOF

  $ umask 077

  $ hg init repo
  $ cd repo

  $ chmod 0770 .hg/store

before commit
store can be written by the group, other files cannot
store is setgid

  $ $PYTHON ../printmodes.py .
  00700 ./.hg/
  00600 ./.hg/00changelog.i
  00600 ./.hg/requires
  00770 ./.hg/store/
  00600 ./.hg/store/requires

  $ mkdir dir
  $ touch foo dir/bar
  $ hg ci -qAm 'add files'

after commit
working dir files can only be written by the owner
files created in .hg can be written by the group
(in particular, store/**, dirstate, branch cache file, undo files)
new directories are setgid

  $ $PYTHON ../printmodes.py .
  00700 ./.hg/
  00600 ./.hg/00changelog.i
  00660 ./.hg/checkoutidentifier
  00660 ./.hg/dirstate
  00660 ./.hg/last-message.txt
  00600 ./.hg/requires
  00770 ./.hg/store/
  00660 ./.hg/store/00changelog.d
  00660 ./.hg/store/00changelog.i
  00660 ./.hg/store/00manifest.i
  02775 ./.hg/store/allheads/
  00664 ./.hg/store/allheads/index-node
  00664 ./.hg/store/allheads/log
  00664 ./.hg/store/allheads/meta
  00770 ./.hg/store/data/
  00770 ./.hg/store/data/dir/
  00660 ./.hg/store/data/dir/bar.i
  00660 ./.hg/store/data/foo.i
  00660 ./.hg/store/fncache
  02775 ./.hg/store/hgcommits/
  02775 ./.hg/store/hgcommits/v1/
  00664 ./.hg/store/hgcommits/v1/index-id
  00664 ./.hg/store/hgcommits/v1/log
  00664 ./.hg/store/hgcommits/v1/meta
  02775 ./.hg/store/metalog/
  02775 ./.hg/store/metalog/blobs/
  00664 ./.hg/store/metalog/blobs/index-id
  00664 ./.hg/store/metalog/blobs/log
  00664 ./.hg/store/metalog/blobs/meta
  02775 ./.hg/store/metalog/roots/
  00664 ./.hg/store/metalog/roots/index-reverse
  00664 ./.hg/store/metalog/roots/log
  00664 ./.hg/store/metalog/roots/meta
  00660 ./.hg/store/phaseroots
  00600 ./.hg/store/requires
  00660 ./.hg/store/undo
  00660 ./.hg/store/undo.backupfiles
  00660 ./.hg/store/undo.bookmarks
  00660 ./.hg/store/undo.phaseroots
  00700 ./.hg/treestate/
  00600 ./.hg/treestate/* (glob)
  00660 ./.hg/undo.backup.dirstate
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

  $ $PYTHON ../printmodes.py ../push
  00770 ../push/.hg/
  00660 ../push/.hg/00changelog.i
  00660 ../push/.hg/requires
  00770 ../push/.hg/store/
  00660 ../push/.hg/store/requires

  $ umask 077
  $ hg -q push ../push

after push
group can still write everything
XXX: treestate and allheads do not really respect this rule

  $ $PYTHON ../printmodes.py ../push
  00770 ../push/.hg/
  00660 ../push/.hg/00changelog.i
  00660 ../push/.hg/dirstate
  00660 ../push/.hg/requires
  00770 ../push/.hg/store/
  00660 ../push/.hg/store/00changelog.d
  00660 ../push/.hg/store/00changelog.i
  00660 ../push/.hg/store/00manifest.i
  02775 ../push/.hg/store/allheads/
  00664 ../push/.hg/store/allheads/index-node
  00664 ../push/.hg/store/allheads/log
  00664 ../push/.hg/store/allheads/meta
  00770 ../push/.hg/store/data/
  00770 ../push/.hg/store/data/dir/
  00660 ../push/.hg/store/data/dir/bar.i
  00660 ../push/.hg/store/data/foo.i
  00660 ../push/.hg/store/fncache
  02775 ../push/.hg/store/hgcommits/
  02775 ../push/.hg/store/hgcommits/v1/
  00664 ../push/.hg/store/hgcommits/v1/index-id
  00664 ../push/.hg/store/hgcommits/v1/log
  00664 ../push/.hg/store/hgcommits/v1/meta
  02775 ../push/.hg/store/metalog/
  02775 ../push/.hg/store/metalog/blobs/
  00664 ../push/.hg/store/metalog/blobs/index-id
  00664 ../push/.hg/store/metalog/blobs/log
  00664 ../push/.hg/store/metalog/blobs/meta
  02775 ../push/.hg/store/metalog/roots/
  00664 ../push/.hg/store/metalog/roots/index-reverse
  00664 ../push/.hg/store/metalog/roots/log
  00664 ../push/.hg/store/metalog/roots/meta
  00660 ../push/.hg/store/requires
  00660 ../push/.hg/store/undo
  00660 ../push/.hg/store/undo.backupfiles
  00660 ../push/.hg/store/undo.bookmarks
  00660 ../push/.hg/store/undo.phaseroots
  00700 ../push/.hg/treestate/
  00600 ../push/.hg/treestate/* (glob)
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
  $ chmod g+s .hg/store 2> /dev/null || true
  $ mkdir dir
  $ touch dir/file
  $ hg ci -qAm 'add dir/file'
  $ storemode=`$PYTHON ../mode.py .hg/store`
  $ dirmode=`$PYTHON ../mode.py .hg/store/data/dir`
  $ if [ "$storemode" != "$dirmode" ]; then
  >  echo "$storemode != $dirmode"
  > fi
  $ cd ..

  $ cd .. # g-s dir

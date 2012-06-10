
  $ . "$TESTDIR/bzr-definitions"
  $ cat > ghostcreator.py <<EOF
  > import sys
  > from bzrlib import workingtree
  > wt = workingtree.WorkingTree.open('.')
  > 
  > message, ghostrev = sys.argv[1:]
  > wt.set_parent_ids(wt.get_parent_ids() + [ghostrev])
  > wt.commit(message)
  > EOF

ghost revisions

  $ mkdir test-ghost-revisions
  $ cd test-ghost-revisions
  $ bzr init -q source
  $ cd source
  $ echo content > somefile
  $ bzr add -q somefile
  $ bzr commit -q -m 'Initial layout setup'
  $ echo morecontent >> somefile
  $ python ../../ghostcreator.py 'Commit with ghost revision' ghostrev
  $ cd ..
  $ hg convert source source-hg
  initializing destination source-hg repository
  scanning source...
  sorting...
  converting...
  1 Initial layout setup
  0 Commit with ghost revision
  $ glog -R source-hg
  o  1@source "Commit with ghost revision" files: somefile
  |
  o  0@source "Initial layout setup" files: somefile
  

  $ cd ..

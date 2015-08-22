#require bzr

  $ . "$TESTDIR/bzr-definitions"
  $ cat > treeset.py <<EOF
  > import sys
  > from bzrlib import workingtree
  > wt = workingtree.WorkingTree.open('.')
  > 
  > message, rootid = sys.argv[1:]
  > wt.set_root_id('tree_root-%s' % rootid)
  > wt.commit(message)
  > EOF

change the id of the tree root

  $ mkdir test-change-treeroot-id
  $ cd test-change-treeroot-id
  $ bzr init -q source
  $ cd source
  $ echo content > file
  $ bzr add -q file
  $ bzr commit -q -m 'Initial add'
  $ python ../../treeset.py 'Changed root' new
  $ cd ..
  $ hg convert source source-hg
  initializing destination source-hg repository
  scanning source...
  sorting...
  converting...
  1 Initial add
  0 Changed root
  $ manifest source-hg tip
  % manifest of tip
  644   file

  $ cd ..

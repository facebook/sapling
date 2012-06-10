test children command

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > children =
  > EOF

init
  $ hg init t
  $ cd t

no working directory
  $ hg children

setup
  $ echo 0 > file0
  $ hg ci -qAm 0 -d '0 0'

  $ echo 1 > file1
  $ hg ci -qAm 1 -d '1 0'

  $ echo 2 >> file0
  $ hg ci -qAm 2 -d '2 0'

  $ hg co null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo 3 > file3
  $ hg ci -qAm 3 -d '3 0'

hg children at revision 3 (tip)
  $ hg children

  $ hg co null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

hg children at nullrev (should be 0 and 3)
  $ hg children
  changeset:   0:4df8521a7374
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  
  changeset:   3:e2962852269d
  tag:         tip
  parent:      -1:000000000000
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     3
  
  $ hg co 1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

hg children at revision 1 (should be 2)
  $ hg children
  changeset:   2:8f5eea5023c2
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     2
  
  $ hg co 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

hg children at revision 2 (other head)
  $ hg children

  $ for i in null 0 1 2 3; do
  > echo "hg children -r $i"
  > hg children -r $i
  > done
  hg children -r null
  changeset:   0:4df8521a7374
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  
  changeset:   3:e2962852269d
  tag:         tip
  parent:      -1:000000000000
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     3
  
  hg children -r 0
  changeset:   1:708c093edef0
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     1
  
  hg children -r 1
  changeset:   2:8f5eea5023c2
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     2
  
  hg children -r 2
  hg children -r 3

hg children -r 0 file0 (should be 2)
  $ hg children -r 0 file0
  changeset:   2:8f5eea5023c2
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     2
  

hg children -r 1 file0 (should be 2)
  $ hg children -r 1 file0
  changeset:   2:8f5eea5023c2
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     2
  

  $ hg co 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

hg children file0 at revision 0 (should be 2)
  $ hg children file0
  changeset:   2:8f5eea5023c2
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     2
  

  $ cd ..

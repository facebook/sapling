  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > remotenames=`dirname $TESTDIR`/remotenames.py
  > [remotenames]
  > alias.default = True
  > EOF

  $ FILTERPWD="sed s%`pwd`/%%g"

  $ mkcommit () {
  >     echo c$1 > f$1
  >     hg add f$1
  >     hg ci -m "$1"
  > }

  $ hg init alpha
  $ cd alpha
  $ mkcommit 0
  $ mkcommit 1
  $ hg branch stable
  marked working directory as branch stable
  (branches are permanent and global, did you want a bookmark?)
  $ mkcommit 2
  $ cd ..
  $ hg clone alpha beta | $FILTERPWD
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd beta
  $ mkcommit 3
  $ hg co -C stable
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merged'
  $ cd ..

  $ hg init gamma
  $ cd gamma

  $ cat > .hg/hgrc <<EOF
  > [paths]
  > default = ../alpha
  > beta = ../beta
  > EOF

  $ hg pull | $FILTERPWD
  pulling from alpha
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  (run 'hg update' to get a working copy)

  $ hg pull beta | $FILTERPWD
  pulling from beta
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)

  $ hg co -C default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg branch default
  marked working directory as branch default

  $ mkcommit 4
  $ hg merge stable
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg ci -m 'merging stable'

  $ hg log
  changeset:   6:af8f9c9afd61
  tag:         tip
  parent:      5:e5922cca29a9
  parent:      4:a43aa1e4a27c
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merging stable
  
  changeset:   5:e5922cca29a9
  parent:      3:5ae9f075bc64
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     4
  
  changeset:   4:a43aa1e4a27c
  branch:      stable
  branch:      beta/stable
  parent:      2:5b35a0d5bd4d
  parent:      3:5ae9f075bc64
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merged
  
  changeset:   3:5ae9f075bc64
  branch:      beta
  parent:      1:2b9c7234e035
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     3
  
  changeset:   2:5b35a0d5bd4d
  branch:      stable
  branch:      default/stable
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  
  changeset:   1:2b9c7234e035
  branch:      default/default
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  changeset:   0:6cee5c8f3e5b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  

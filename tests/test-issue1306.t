http://mercurial.selenic.com/bts/issue1306

Initialize remote repo with branches:

  $ hg init remote
  $ cd remote

  $ echo a > a
  $ hg ci -Ama
  adding a

  $ hg branch br
  marked working directory as branch br
  $ hg ci -Amb

  $ echo c > c
  $ hg ci -Amc
  adding c

  $ hg log
  changeset:   2:ae3d9c30ec50
  branch:      br
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
  changeset:   1:3f7f930ca414
  branch:      br
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  

  $ cd ..

Try cloning -r branch:

  $ hg clone -rbr remote local1
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 2 files
  updating to branch br
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg -R local1 parents
  changeset:   2:ae3d9c30ec50
  branch:      br
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  

Try cloning -rother clone#branch:

  $ hg clone -r0 remote#br local2
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 2 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg -R local2 parents
  changeset:   0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  

Try cloning -r1 clone#branch:

  $ hg clone -r1 remote#br local3
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 2 files
  updating to branch br
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg -R local3 parents
  changeset:   1:3f7f930ca414
  branch:      br
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  


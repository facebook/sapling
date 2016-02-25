  $ hg init a
  $ cd a
  $ echo a > a
  $ hg ci -Am0
  adding a
  $ echo b > b
  $ hg ci -Am1
  adding b
  $ hg tag -r0 default
  warning: tag default conflicts with existing branch name
  $ hg log
  changeset:   2:30a83d1e4a1e
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag default for changeset f7b1eb17ad24
  
  changeset:   1:925d80f479bb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  changeset:   0:f7b1eb17ad24
  tag:         default
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  
  $ hg update 'tag(default)'
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg parents
  changeset:   0:f7b1eb17ad24
  tag:         default
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  
  $ hg update 'branch(default)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg parents
  changeset:   2:30a83d1e4a1e
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag default for changeset f7b1eb17ad24
  

  $ cd ..

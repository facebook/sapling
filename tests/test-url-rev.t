Test basic functionality of url#rev syntax

  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg ci -qAm 'add a'
  $ hg branch foo
  marked working directory as branch foo
  $ echo >> a
  $ hg ci -m 'change a'
  $ cd ..

  $ hg clone 'repo#foo' clone
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  updating to branch foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg --cwd clone heads
  changeset:   1:cd2a86ecc814
  branch:      foo
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change a
  
  changeset:   0:1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  
  $ hg --cwd clone parents
  changeset:   1:cd2a86ecc814
  branch:      foo
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change a
  
  $ cat clone/.hg/hgrc
  [paths]
  default = $TESTTMP/repo#foo

Changing original repo:

  $ cd repo

  $ echo >> a
  $ hg ci -m 'new head of branch foo'

  $ hg up -qC default
  $ echo bar > bar
  $ hg ci -qAm 'add bar'

  $ hg log
  changeset:   3:4cd725637392
  tag:         tip
  parent:      0:1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add bar
  
  changeset:   2:faba9097cad4
  branch:      foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     new head of branch foo
  
  changeset:   1:cd2a86ecc814
  branch:      foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change a
  
  changeset:   0:1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  
  $ hg -q outgoing '../clone#foo'
  2:faba9097cad4

  $ hg -q push '../clone#foo'

  $ hg --cwd ../clone heads
  changeset:   2:faba9097cad4
  branch:      foo
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     new head of branch foo
  
  changeset:   0:1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  
  $ cd ..

  $ cd clone
  $ hg rollback
  repository tip rolled back to revision 1 (undo push)

  $ hg -q incoming
  2:faba9097cad4

  $ hg -q pull

  $ hg heads
  changeset:   2:faba9097cad4
  branch:      foo
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     new head of branch foo
  
  changeset:   0:1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  
Pull should not have updated:

  $ hg parents -q
  1:cd2a86ecc814

Going back to the default branch:

  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg parents
  changeset:   0:1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  
No new revs, no update:

  $ hg pull -qu

  $ hg parents -q
  0:1f0dee641bb7

  $ hg rollback
  repository tip rolled back to revision 1 (undo pull)

  $ hg parents -q
  0:1f0dee641bb7

Pull -u takes us back to branch foo:

  $ hg pull -qu

  $ hg parents
  changeset:   2:faba9097cad4
  branch:      foo
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     new head of branch foo
  
  $ hg rollback
  repository tip rolled back to revision 1 (undo pull)
  working directory now based on revision 0

  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg parents -q
  0:1f0dee641bb7

  $ hg heads -q
  1:cd2a86ecc814
  0:1f0dee641bb7

  $ hg pull -qur default default

  $ hg parents
  changeset:   3:4cd725637392
  tag:         tip
  parent:      0:1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add bar
  
  $ hg heads
  changeset:   3:4cd725637392
  tag:         tip
  parent:      0:1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add bar
  
  changeset:   2:faba9097cad4
  branch:      foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     new head of branch foo
  
Test handling of invalid urls

  $ hg id http://foo/?bar
  abort: unsupported URL component: "bar"
  [255]

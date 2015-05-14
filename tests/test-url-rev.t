Test basic functionality of url#rev syntax

  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg ci -qAm 'add a'
  $ hg branch foo
  marked working directory as branch foo
  (branches are permanent and global, did you want a bookmark?)
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
  # example repository config (see "hg help config" for more info)
  [paths]
  default = $TESTTMP/repo#foo (glob)
  
  # path aliases to other clones of this repo in URLs or filesystem paths
  # (see "hg help config.paths" for more info)
  #
  # default-push = ssh://jdoe@example.net/hg/jdoes-fork
  # my-fork      = ssh://jdoe@example.net/hg/jdoes-fork
  # my-clone     = /home/jdoe/jdoes-clone
  
  [ui]
  # name and email (local to this repository, optional), e.g.
  # username = Jane Doe <jdoe@example.com>

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
  
  $ hg -q outgoing '../clone'
  2:faba9097cad4
  3:4cd725637392
  $ hg summary --remote --config paths.default='../clone'
  parent: 3:4cd725637392 tip
   add bar
  branch: default
  commit: (clean)
  update: (current)
  phases: 4 draft (draft)
  remote: 2 outgoing
  $ hg -q outgoing '../clone#foo'
  2:faba9097cad4
  $ hg summary --remote --config paths.default='../clone#foo'
  parent: 3:4cd725637392 tip
   add bar
  branch: default
  commit: (clean)
  update: (current)
  phases: 4 draft (draft)
  remote: 1 outgoing

  $ hg -q --cwd ../clone incoming '../repo#foo'
  2:faba9097cad4
  $ hg --cwd ../clone summary --remote --config paths.default='../repo#foo'
  parent: 1:cd2a86ecc814 tip
   change a
  branch: foo
  commit: (clean)
  update: (current)
  remote: 1 or more incoming

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
  
  $ hg -q --cwd ../clone incoming '../repo#foo'
  [1]
  $ hg --cwd ../clone summary --remote --config paths.default='../repo#foo'
  parent: 1:cd2a86ecc814 
   change a
  branch: foo
  commit: (clean)
  update: 1 new changesets (update)
  remote: (synced)

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

  $ cd ..

Test handling common incoming revisions between "default" and
"default-push"

  $ hg -R clone rollback
  repository tip rolled back to revision 1 (undo pull)
  working directory now based on revision 0

  $ cd repo

  $ hg update -q -C default
  $ echo modified >> bar
  $ hg commit -m "new head to push current default head"
  $ hg -q push -r ".^1" '../clone'

  $ hg -q outgoing '../clone'
  2:faba9097cad4
  4:d515801a8f3d

  $ hg summary --remote --config paths.default='../clone#default' --config paths.default-push='../clone#foo'
  parent: 4:d515801a8f3d tip
   new head to push current default head
  branch: default
  commit: (clean)
  update: (current)
  phases: 1 draft (draft)
  remote: 1 outgoing

  $ hg summary --remote --config paths.default='../clone#foo' --config paths.default-push='../clone'
  parent: 4:d515801a8f3d tip
   new head to push current default head
  branch: default
  commit: (clean)
  update: (current)
  phases: 1 draft (draft)
  remote: 2 outgoing

  $ hg summary --remote --config paths.default='../clone' --config paths.default-push='../clone#foo'
  parent: 4:d515801a8f3d tip
   new head to push current default head
  branch: default
  commit: (clean)
  update: (current)
  phases: 1 draft (draft)
  remote: 1 outgoing

  $ hg clone -q -r 0 . ../another
  $ hg -q outgoing '../another#default'
  3:4cd725637392
  4:d515801a8f3d

  $ hg summary --remote --config paths.default='../another#default' --config paths.default-push='../clone#default'
  parent: 4:d515801a8f3d tip
   new head to push current default head
  branch: default
  commit: (clean)
  update: (current)
  phases: 1 draft (draft)
  remote: 1 outgoing

  $ cd ..

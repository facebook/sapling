#chg-compatible

  $ setconfig extensions.treemanifest=!
Test basic functionality of url#rev syntax

  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg ci -qAm 'add a'
  $ echo >> a
  $ hg ci -m 'change a'
  $ hg bookmark foo
  $ cd ..

  $ hg clone 'repo#foo' clone
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg --cwd clone heads
  changeset:   1:17d330177ee9
  bookmark:    foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change a
  
  $ hg --cwd clone parents
  changeset:   1:17d330177ee9
  bookmark:    foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change a
  
  $ cat clone/.hg/hgrc
  # example repository config (see 'hg help config' for more info)
  [paths]
  default = $TESTTMP/repo#foo
  
  # path aliases to other clones of this repo in URLs or filesystem paths
  # (see 'hg help config.paths' for more info)
  #
  # default:pushurl = ssh://jdoe@example.net/hg/jdoes-fork
  # my-fork         = ssh://jdoe@example.net/hg/jdoes-fork
  # my-clone        = /home/jdoe/jdoes-clone
  
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
  changeset:   3:ad4513930219
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add bar
  
  changeset:   2:7d4251d04d20
  bookmark:    foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     new head of branch foo
  
  changeset:   1:17d330177ee9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change a
  
  changeset:   0:1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  
  $ hg -q outgoing '../clone'
  2:7d4251d04d20
  3:ad4513930219
  $ hg summary --remote --config paths.default='../clone'
  parent: 3:ad4513930219 
   add bar
  commit: (clean)
  phases: 4 draft
  remote: 2 outgoing
  $ hg -q outgoing '../clone#foo'
  2:7d4251d04d20
  $ hg summary --remote --config paths.default='../clone#foo'
  parent: 3:ad4513930219 
   add bar
  commit: (clean)
  phases: 4 draft
  remote: 1 outgoing

  $ hg -q --cwd ../clone incoming '../repo#foo'
  2:7d4251d04d20
  $ hg --cwd ../clone summary --remote --config paths.default='../repo#foo'
  parent: 1:17d330177ee9 
   change a
  bookmarks: foo
  commit: (clean)
  remote: 1 or more incoming

  $ hg -q push '../clone#foo'

  $ hg --cwd ../clone heads
  changeset:   2:7d4251d04d20
  bookmark:    foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     new head of branch foo
  
  $ hg -q --cwd ../clone incoming '../repo#foo'
  $ hg --cwd ../clone summary --remote --config paths.default='../repo#foo'
  parent: 1:17d330177ee9 
   change a
  commit: (clean)
  remote: (synced)

  $ cd ..

  $ cd clone
  $ hg rollback
  repository tip rolled back to revision 1 (undo push)

  $ hg -q incoming
  2:7d4251d04d20

  $ hg -q pull

  $ hg heads
  changeset:   2:7d4251d04d20
  bookmark:    foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     new head of branch foo
  
Pull should not have updated:

  $ hg parents -q
  1:17d330177ee9

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

  $ hg debugstrip 1 --no-backup

  $ hg parents -q
  0:1f0dee641bb7

Pull -u takes us back to branch foo:

  $ hg pull -qu

  $ hg parents
  changeset:   2:7d4251d04d20
  bookmark:    foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     new head of branch foo
  
  $ hg debugstrip 2 --no-backup
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark foo)

  $ hg parents -q
  0:1f0dee641bb7

  $ hg heads -q
  1:17d330177ee9

  $ hg pull -qur default default

  $ hg parents
  changeset:   3:ad4513930219
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add bar
  
  $ hg heads
  changeset:   3:ad4513930219
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add bar
  
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
  4:44b4e0c07491

  $ hg summary --remote --config paths.default='../clone#foo' --config paths.default-push='../clone'
  parent: 4:44b4e0c07491 
   new head to push current default head
  commit: (clean)
  phases: 1 draft
  remote: 1 outgoing

  $ hg summary --remote --config paths.default='../clone' --config paths.default-push='../clone#foo'
  parent: 4:44b4e0c07491 
   new head to push current default head
  commit: (clean)
  phases: 1 draft
  remote: (synced)

  $ cd ..

Test url#rev syntax of local destination path, which should be taken as
a 'url#rev' path

  $ hg clone repo '#foo'
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg root -R '#foo'
  $TESTTMP/#foo

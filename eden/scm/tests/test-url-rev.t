#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ disable treemanifest
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
  commit:      17d330177ee9
  bookmark:    foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change a
  
  $ hg --cwd clone parents
  commit:      17d330177ee9
  bookmark:    foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change a
  
  $ cat clone/.hg/hgrc
  # example repository config (see 'hg help config' for more info)
  [paths]
  default = $TESTTMP/repo#foo
  
  # URL aliases to other repo sources
  # (see 'hg help config.paths' for more info)
  #
  # my-fork = https://example.com/jdoe/example-repo
  
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
  commit:      ad4513930219
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add bar
  
  commit:      7d4251d04d20
  bookmark:    foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     new head of branch foo
  
  commit:      17d330177ee9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change a
  
  commit:      1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  
  $ hg summary --remote --config paths.default='../clone'
  parent: ad4513930219 
   add bar
  commit: (clean)
  phases: 4 draft
  remote: 2 outgoing
  $ hg summary --remote --config paths.default='../clone#foo'
  parent: ad4513930219 
   add bar
  commit: (clean)
  phases: 4 draft
  remote: 1 outgoing

  $ hg --cwd ../clone summary --remote --config paths.default='../repo#foo'
  parent: 17d330177ee9 
   change a
  bookmarks: foo
  commit: (clean)
  phases: 2 draft
  remote: 1 or more incoming

  $ hg -q push '../clone#foo'

  $ hg --cwd ../clone heads
  commit:      7d4251d04d20
  bookmark:    foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     new head of branch foo
  
  $ hg --cwd ../clone summary --remote --config paths.default='../repo#foo'
  parent: 17d330177ee9 
   change a
  commit: (clean)
  phases: 3 draft
  remote: (synced)

  $ cd ..

  $ cd clone

  $ hg -q pull

  $ hg heads
  commit:      7d4251d04d20
  bookmark:    foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     new head of branch foo
  
Pull should not have updated:

  $ hg parents -q
  17d330177ee9

Going back to the default branch:

  $ hg up -C 'desc(add)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg parents
  commit:      1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  
No new revs, no update:

  $ hg pull -qu

  $ hg parents -q
  7d4251d04d20

  $ hg debugstrip -q 'desc(change)' --no-backup

  $ hg parents -q
  1f0dee641bb7

Pull -u takes us back to branch foo:

  $ hg pull -qu

  $ hg parents
  commit:      7d4251d04d20
  bookmark:    foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     new head of branch foo
  
  $ hg debugstrip 'desc(new)' --no-backup
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg up -C 'desc(add)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark foo)

  $ hg parents -q
  1f0dee641bb7

  $ hg heads -q
  17d330177ee9

  $ hg pull -qur default default

  $ hg parents
  commit:      ad4513930219
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add bar
  
  $ hg heads
  commit:      ad4513930219
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add bar
  
  $ cd ..

Test handling common incoming revisions between "default" and
"default-push"

  $ cd repo

  $ hg goto -q -C default
  $ echo modified >> bar
  $ hg commit -m "new head to push current default head"

  $ hg summary --remote --config paths.default='../clone#foo' --config paths.default-push='../clone'
  parent: 44b4e0c07491 
   new head to push current default head
  commit: (clean)
  phases: 5 draft
  remote: 1 outgoing

  $ hg summary --remote --config paths.default='../clone' --config paths.default-push='../clone#foo'
  parent: 44b4e0c07491 
   new head to push current default head
  commit: (clean)
  phases: 5 draft
  remote: (synced)

  $ cd ..

Test url#rev syntax of local destination path, which should be taken as
a 'url#rev' path

  $ hg clone repo '#foo'
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg root -R '#foo'
  $TESTTMP/#foo

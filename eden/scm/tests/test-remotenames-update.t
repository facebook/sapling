#debugruntest-compatible

#require no-eden


  $ eagerepo
  $ setconfig devel.segmented-changelog-rev-compat=true
Set up repo

  $ enable remotenames

  $ hg init repo
  $ cd repo
  $ echo 'foo'> a.txt
  $ hg add a.txt
  $ hg commit -m "a"
  $ echo 'bar' > b.txt
  $ hg add b.txt
  $ hg commit -m "b"
  $ hg bookmark foo -i
  $ echo 'bar' > c.txt
  $ hg add c.txt
  $ hg commit -q -m "c"

Testing update -B feature

  $ hg log -G -T '{bookmarks} {remotebookmarks}'
  @
  │
  o  foo
  │
  o
  

  $ hg goto -B bar foo
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark foo)
  $ hg log -G -T '{bookmarks} {remotebookmarks}'
  o
  │
  @  bar foo
  │
  o
  
  $ hg bookmarks -v
   * bar                       661086655130            [foo]
     foo                       661086655130

  $ hg goto -B foo bar
  abort: bookmark 'foo' already exists
  [255]


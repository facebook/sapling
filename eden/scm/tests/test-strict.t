#debugruntest-compatible

#require no-eden


  $ eagerepo
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ hg init repo
  $ cd repo

  $ echo a > a
  $ hg ci -Ama
  adding a

  $ hg an a
  cb9a9f314b8b: a

  $ hg --config ui.strict=False an a
  cb9a9f314b8b: a

  $ setconfig ui.strict=true

No difference - "an" is an alias

  $ hg an a
  cb9a9f314b8b: a
  $ hg annotate a
  cb9a9f314b8b: a

should succeed - up is an alias, not an abbreviation

  $ hg up tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

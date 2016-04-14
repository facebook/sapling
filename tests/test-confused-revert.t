  $ hg init
  $ echo foo > a
  $ hg add a
  $ hg commit -m "1"

  $ echo bar > b
  $ hg add b
  $ hg remove a

Should show a removed and b added:

  $ hg status
  A b
  R a

  $ hg revert --all
  undeleting a
  forgetting b

Should show b unknown and a back to normal:

  $ hg status
  ? b

  $ rm b

  $ hg co -C 0
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo foo-a > a
  $ hg commit -m "2a"

  $ hg co -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo foo-b > a
  $ hg commit -m "2b"
  created new head

  $ HGMERGE=true hg merge 1
  merging a
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

Should show foo-b:

  $ cat a
  foo-b

  $ echo bar > b
  $ hg add b
  $ rm a
  $ hg remove a

Should show a removed and b added:

  $ hg status
  A b
  R a

Revert should fail:

  $ hg revert
  abort: uncommitted merge with no revision specified
  (use 'hg update' or see 'hg help revert')
  [255]

Revert should be ok now:

  $ hg revert -r2 --all
  undeleting a
  forgetting b

Should show b unknown and a marked modified (merged):

  $ hg status
  M a
  ? b

Should show foo-b:

  $ cat a
  foo-b


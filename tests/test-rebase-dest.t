Require a destination
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase =
  > [commands]
  > rebase.requiredest = True
  > EOF
  $ hg init repo
  $ cd repo
  $ echo a >> a
  $ hg commit -qAm aa
  $ echo b >> b
  $ hg commit -qAm bb
  $ hg up ".^"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c >> c
  $ hg commit -qAm cc
  $ hg rebase
  abort: you must specify a destination
  (use: hg rebase -d REV)
  [255]
  $ hg rebase -d 1
  rebasing 2:5db65b93a12b "cc" (tip)
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/5db65b93a12b-4fb789ec-backup.hg (glob)
  $ hg rebase -d 0 -r . -q
  $ HGPLAIN=1 hg rebase
  rebasing 2:889b0bc6a730 "cc" (tip)
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/889b0bc6a730-41ec4f81-backup.hg (glob)
  $ hg rebase -d 0 -r . -q
  $ hg --config commands.rebase.requiredest=False rebase
  rebasing 2:279de9495438 "cc" (tip)
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/279de9495438-ab0a5128-backup.hg (glob)


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
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/5db65b93a12b-4fb789ec-rebase.hg (glob)
  $ hg rebase -d 0 -r . -q
  $ HGPLAIN=1 hg rebase
  rebasing 2:889b0bc6a730 "cc" (tip)
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/889b0bc6a730-41ec4f81-rebase.hg (glob)
  $ hg rebase -d 0 -r . -q
  $ hg --config commands.rebase.requiredest=False rebase
  rebasing 2:279de9495438 "cc" (tip)
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/279de9495438-ab0a5128-rebase.hg (glob)

Requiring dest should not break continue or other rebase options
  $ hg up 1 -q
  $ echo d >> c
  $ hg commit -qAm dc
  $ hg log -G -T '{rev} {desc}'
  @  3 dc
  |
  | o  2 cc
  |/
  o  1 bb
  |
  o  0 aa
  
  $ hg rebase -d 2
  rebasing 3:0537f6b50def "dc" (tip)
  merging c
  warning: conflicts while merging c! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ echo d > c
  $ hg resolve --mark --all
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing 3:0537f6b50def "dc" (tip)
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/0537f6b50def-be4c7386-rebase.hg (glob)

  $ cd ..

Check rebase.requiredest interaction with pull --rebase
  $ hg clone repo clone
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo
  $ echo e > e
  $ hg commit -qAm ee
  $ cd ..
  $ cd clone
  $ echo f > f
  $ hg commit -qAm ff
  $ hg pull --rebase
  abort: rebase destination required by configuration
  (use hg pull followed by hg rebase -d DEST)
  [255]


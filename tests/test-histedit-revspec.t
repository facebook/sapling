
Enable extensions used by this test.
  $ cat >>$HGRCPATH <<EOF
  > [extensions]
  > histedit=
  > EOF

Repo setup.
  $ hg init foo
  $ cd foo
  $ echo alpha >> alpha
  $ hg addr
  adding alpha
  $ hg ci -m one
  $ echo alpha >> alpha
  $ hg ci -m two
  $ echo alpha >> alpha
  $ hg ci -m three
  $ echo alpha >> alpha
  $ hg ci -m four
  $ echo alpha >> alpha
  $ hg ci -m five

  $ hg log --style compact --graph
  @  4[tip]   08d98a8350f3   1970-01-01 00:00 +0000   test
  |    five
  |
  o  3   c8e68270e35a   1970-01-01 00:00 +0000   test
  |    four
  |
  o  2   eb57da33312f   1970-01-01 00:00 +0000   test
  |    three
  |
  o  1   579e40513370   1970-01-01 00:00 +0000   test
  |    two
  |
  o  0   6058cbb6cfd7   1970-01-01 00:00 +0000   test
       one
  

Run a dummy edit to make sure we get tip^^ correctly via revsingle.
  $ HGEDITOR=cat hg histedit "tip^^"
  pick eb57da33312f 2 three
  pick c8e68270e35a 3 four
  pick 08d98a8350f3 4 five
  
  # Edit history between eb57da33312f and 08d98a8350f3
  #
  # Commands:
  #  p, pick = use commit
  #  e, edit = use commit, but stop for amending
  #  f, fold = use commit, but fold into previous commit (combines N and N-1)
  #  d, drop = remove commit from history
  #  m, mess = edit message without changing commit content
  #
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

Run on a revision not ancestors of the current working directory.

  $ hg up 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg histedit -r 4
  abort: 08d98a8350f3 is not an ancestor of working directory
  [255]

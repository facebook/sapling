Test argument handling and various data parsing
==================================================


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
  

histedit --continue/--abort with no existing state
--------------------------------------------------

  $ hg histedit --continue
  abort: no histedit in progress
  [255]
  $ hg histedit --abort
  abort: no histedit in progress
  [255]

Run a dummy edit to make sure we get tip^^ correctly via revsingle.
--------------------------------------------------------------------

  $ HGEDITOR=cat hg histedit "tip^^"
  pick eb57da33312f 2 three
  pick c8e68270e35a 3 four
  pick 08d98a8350f3 4 five
  
  # Edit history between eb57da33312f and 08d98a8350f3
  #
  # Commits are listed from least to most recent
  #
  # Commands:
  #  p, pick = use commit
  #  e, edit = use commit, but stop for amending
  #  f, fold = use commit, but combine it with the one above
  #  r, roll = like fold, but discard this commit's description
  #  d, drop = remove commit from history
  #  m, mess = edit message without changing commit content
  #
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

Run on a revision not ancestors of the current working directory.
--------------------------------------------------------------------

  $ hg up 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg histedit -r 4
  abort: 08d98a8350f3 is not an ancestor of working directory
  [255]
  $ hg up --quiet


Test that we pick the minimum of a revrange
---------------------------------------

  $ HGEDITOR=cat hg histedit '2::' --commands - << EOF
  > pick eb57da33312f 2 three
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up --quiet

  $ HGEDITOR=cat hg histedit 'tip:2' --commands - << EOF
  > pick eb57da33312f 2 three
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up --quiet

Test config specified default
-----------------------------

  $ HGEDITOR=cat hg histedit --config "histedit.defaultrev=only(.) - ::eb57da33312f" --commands - << EOF
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

Run on a revision not descendants of the initial parent
--------------------------------------------------------------------

Test the message shown for inconsistent histedit state, which may be
created (and forgotten) by Mercurial earlier than 2.7. This emulates
Mercurial earlier than 2.7 by renaming ".hg/histedit-state"
temporarily.

  $ HGEDITOR=cat hg histedit -r 4 --commands - << EOF
  > edit 08d98a8350f3 4 five
  > EOF
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  reverting alpha
  Make changes as needed, you may commit or record as needed now.
  When you are finished, run hg histedit --continue to resume.
  [1]

  $ mv .hg/histedit-state .hg/histedit-state.back
  $ hg update --quiet --clean 2
  $ mv .hg/histedit-state.back .hg/histedit-state

  $ hg histedit --continue
  abort: c8e68270e35a is not an ancestor of working directory
  (use "histedit --abort" to clear broken state)
  [255]

  $ hg histedit --abort
  $ hg update --quiet --clean

Test that missing revisions are detected
---------------------------------------

  $ HGEDITOR=cat hg histedit "tip^^" --commands - << EOF
  > pick eb57da33312f 2 three
  > pick 08d98a8350f3 4 five
  > EOF
  abort: missing rules for changeset c8e68270e35a
  (do you want to use the drop action?)
  [255]

Test that extra revisions are detected
---------------------------------------

  $ HGEDITOR=cat hg histedit "tip^^" --commands - << EOF
  > pick 6058cbb6cfd7 0 one
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  abort: may not use changesets other than the ones listed
  [255]

Test malformed line
---------------------------------------

  $ HGEDITOR=cat hg histedit "tip^^" --commands - << EOF
  > pickeb57da33312f2three
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  abort: malformed line "pickeb57da33312f2three"
  [255]

Test unknown changeset
---------------------------------------

  $ HGEDITOR=cat hg histedit "tip^^" --commands - << EOF
  > pick 0123456789ab 2 three
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  abort: unknown changeset 0123456789ab listed
  [255]

Test unknown command
---------------------------------------

  $ HGEDITOR=cat hg histedit "tip^^" --commands - << EOF
  > coin eb57da33312f 2 three
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  abort: unknown action "coin"
  [255]

Test duplicated changeset
---------------------------------------

So one is missing and one appear twice.

  $ HGEDITOR=cat hg histedit "tip^^" --commands - << EOF
  > pick eb57da33312f 2 three
  > pick eb57da33312f 2 three
  > pick 08d98a8350f3 4 five
  > EOF
  abort: duplicated command for changeset eb57da33312f
  [255]

Test short version of command
---------------------------------------

Note: we use varying amounts of white space between command name and changeset
short hash. This tests issue3893.

  $ HGEDITOR=cat hg histedit "tip^^" --commands - << EOF
  > pick eb57da33312f 2 three
  > p    c8e68270e35a 3 four
  > f 08d98a8350f3 4 five
  > EOF
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  reverting alpha
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  four
  ***
  five
  
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: changed alpha
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/foo/.hg/strip-backup/*-backup.hg (glob)

  $ hg update -q 2
  $ echo x > x
  $ hg add x
  $ hg commit -m'x' x
  created new head
  $ hg histedit -r 'heads(all())'
  abort: The specified revisions must have exactly one common root
  [255]

Test that trimming description using multi-byte characters
--------------------------------------------------------------------

  $ python <<EOF
  > fp = open('logfile', 'w')
  > fp.write('12345678901234567890123456789012345678901234567890' +
  >          '12345') # there are 5 more columns for 80 columns
  > 
  > # 2 x 4 = 8 columns, but 3 x 4 = 12 bytes
  > fp.write(u'\u3042\u3044\u3046\u3048'.encode('utf-8'))
  > 
  > fp.close()
  > EOF
  $ echo xx >> x
  $ hg --encoding utf-8 commit --logfile logfile

  $ HGEDITOR=cat hg --encoding utf-8 histedit tip
  pick 3d3ea1f3a10b 5 1234567890123456789012345678901234567890123456789012345\xe3\x81\x82... (esc)
  
  # Edit history between 3d3ea1f3a10b and 3d3ea1f3a10b
  #
  # Commits are listed from least to most recent
  #
  # Commands:
  #  p, pick = use commit
  #  e, edit = use commit, but stop for amending
  #  f, fold = use commit, but combine it with the one above
  #  r, roll = like fold, but discard this commit's description
  #  d, drop = remove commit from history
  #  m, mess = edit message without changing commit content
  #
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

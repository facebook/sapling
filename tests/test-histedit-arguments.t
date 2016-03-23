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
  # You can reorder changesets by reordering the lines
  #
  # Commands:
  #
  #  e, edit = use commit, but stop for amending
  #  m, mess = edit commit message without changing commit content
  #  p, pick = use commit
  #  d, drop = remove commit from history
  #  f, fold = use commit, but combine it with the one above
  #  r, roll = like fold, but discard this commit's description
  #

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
  $ hg up --quiet

  $ HGEDITOR=cat hg histedit 'tip:2' --commands - << EOF
  > pick eb57da33312f 2 three
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  $ hg up --quiet

Test config specified default
-----------------------------

  $ HGEDITOR=cat hg histedit --config "histedit.defaultrev=only(.) - ::eb57da33312f" --commands - << EOF
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF

Run on a revision not descendants of the initial parent
--------------------------------------------------------------------

Test the message shown for inconsistent histedit state, which may be
created (and forgotten) by Mercurial earlier than 2.7. This emulates
Mercurial earlier than 2.7 by renaming ".hg/histedit-state"
temporarily.

  $ hg log -G -T '{rev} {shortest(node)} {desc}\n' -r 2::
  @  4 08d9 five
  |
  o  3 c8e6 four
  |
  o  2 eb57 three
  |
  ~
  $ HGEDITOR=cat hg histedit -r 4 --commands - << EOF
  > edit 08d98a8350f3 4 five
  > EOF
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  reverting alpha
  Editing (08d98a8350f3), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]

  $ hg graft --continue
  abort: no graft in progress
  (continue: hg histedit --continue)
  [255]

  $ mv .hg/histedit-state .hg/histedit-state.back
  $ hg update --quiet --clean 2
  $ echo alpha >> alpha
  $ mv .hg/histedit-state.back .hg/histedit-state

  $ hg histedit --continue
  saved backup bundle to $TESTTMP/foo/.hg/strip-backup/08d98a8350f3-02594089-backup.hg (glob)
  $ hg log -G -T '{rev} {shortest(node)} {desc}\n' -r 2::
  @  4 f5ed five
  |
  | o  3 c8e6 four
  |/
  o  2 eb57 three
  |
  ~

  $ hg unbundle -q $TESTTMP/foo/.hg/strip-backup/08d98a8350f3-02594089-backup.hg
  $ hg strip -q -r f5ed --config extensions.strip=
  $ hg up -q 08d98a8350f3

Test that missing revisions are detected
---------------------------------------

  $ HGEDITOR=cat hg histedit "tip^^" --commands - << EOF
  > pick eb57da33312f 2 three
  > pick 08d98a8350f3 4 five
  > EOF
  hg: parse error: missing rules for changeset c8e68270e35a
  (use "drop c8e68270e35a" to discard, see also: "hg help -e histedit.config")
  [255]

Test that extra revisions are detected
---------------------------------------

  $ HGEDITOR=cat hg histedit "tip^^" --commands - << EOF
  > pick 6058cbb6cfd7 0 one
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  hg: parse error: pick "6058cbb6cfd7" changeset was not a candidate
  (only use listed changesets)
  [255]

Test malformed line
---------------------------------------

  $ HGEDITOR=cat hg histedit "tip^^" --commands - << EOF
  > pickeb57da33312f2three
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  hg: parse error: malformed line "pickeb57da33312f2three"
  [255]

Test unknown changeset
---------------------------------------

  $ HGEDITOR=cat hg histedit "tip^^" --commands - << EOF
  > pick 0123456789ab 2 three
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  hg: parse error: unknown changeset 0123456789ab listed
  [255]

Test unknown command
---------------------------------------

  $ HGEDITOR=cat hg histedit "tip^^" --commands - << EOF
  > coin eb57da33312f 2 three
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  hg: parse error: unknown action "coin"
  [255]

Test duplicated changeset
---------------------------------------

So one is missing and one appear twice.

  $ HGEDITOR=cat hg histedit "tip^^" --commands - << EOF
  > pick eb57da33312f 2 three
  > pick eb57da33312f 2 three
  > pick 08d98a8350f3 4 five
  > EOF
  hg: parse error: duplicated command for changeset eb57da33312f
  [255]

Test bogus rev
---------------------------------------

  $ HGEDITOR=cat hg histedit "tip^^" --commands - << EOF
  > pick eb57da33312f 2 three
  > pick 0
  > pick 08d98a8350f3 4 five
  > EOF
  hg: parse error: invalid changeset 0
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
  four
  ***
  five
  
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: changed alpha
  saved backup bundle to $TESTTMP/foo/.hg/strip-backup/*-backup.hg (glob)
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
  # You can reorder changesets by reordering the lines
  #
  # Commands:
  #
  #  e, edit = use commit, but stop for amending
  #  m, mess = edit commit message without changing commit content
  #  p, pick = use commit
  #  d, drop = remove commit from history
  #  f, fold = use commit, but combine it with the one above
  #  r, roll = like fold, but discard this commit's description
  #

Test --continue with --keep

  $ hg strip -q -r . --config extensions.strip=
  $ hg histedit '.^' -q --keep --commands - << EOF
  > edit eb57da33312f 2 three
  > pick f3cfcca30c44 4 x
  > EOF
  Editing (eb57da33312f), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]
  $ echo edit >> alpha
  $ hg histedit -q --continue
  $ hg log -G -T '{rev}:{node|short} {desc}'
  @  6:8fda0c726bf2 x
  |
  o  5:63379946892c three
  |
  | o  4:f3cfcca30c44 x
  | |
  | | o  3:2a30f3cfee78 four
  | |/   ***
  | |    five
  | o  2:eb57da33312f three
  |/
  o  1:579e40513370 two
  |
  o  0:6058cbb6cfd7 one
  

Test that abort fails gracefully on exception
----------------------------------------------
  $ hg histedit . -q --commands - << EOF
  > edit 8fda0c726bf2 6 x
  > EOF
  Editing (8fda0c726bf2), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]
Corrupt histedit state file
  $ sed 's/8fda0c726bf2/123456789012/' .hg/histedit-state > ../corrupt-histedit
  $ mv ../corrupt-histedit .hg/histedit-state
  $ hg histedit --abort
  warning: encountered an exception during histedit --abort; the repository may not have been completely cleaned up
  abort: .*(No such file or directory:|The system cannot find the file specified).* (re)
  [255]
Histedit state has been exited
  $ hg summary -q
  parent: 5:63379946892c 
  commit: 1 added, 1 unknown (new branch head)
  update: 4 new changesets (update)

  $ cd ..

Set up default base revision tests

  $ hg init defaultbase
  $ cd defaultbase
  $ touch foo
  $ hg -q commit -A -m root
  $ echo 1 > foo
  $ hg commit -m 'public 1'
  $ hg phase --force --public -r .
  $ echo 2 > foo
  $ hg commit -m 'draft after public'
  $ hg -q up -r 1
  $ echo 3 > foo
  $ hg commit -m 'head 1 public'
  created new head
  $ hg phase --force --public -r .
  $ echo 4 > foo
  $ hg commit -m 'head 1 draft 1'
  $ echo 5 > foo
  $ hg commit -m 'head 1 draft 2'
  $ hg -q up -r 2
  $ echo 6 > foo
  $ hg commit -m 'head 2 commit 1'
  $ echo 7 > foo
  $ hg commit -m 'head 2 commit 2'
  $ hg -q up -r 2
  $ echo 8 > foo
  $ hg commit -m 'head 3'
  created new head
  $ hg -q up -r 2
  $ echo 9 > foo
  $ hg commit -m 'head 4'
  created new head
  $ hg merge --tool :local -r 8
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m 'merge head 3 into head 4'
  $ echo 11 > foo
  $ hg commit -m 'commit 1 after merge'
  $ echo 12 > foo
  $ hg commit -m 'commit 2 after merge'

  $ hg log -G -T '{rev}:{node|short} {phase} {desc}\n'
  @  12:8cde254db839 draft commit 2 after merge
  |
  o  11:6f2f0241f119 draft commit 1 after merge
  |
  o    10:90506cc76b00 draft merge head 3 into head 4
  |\
  | o  9:f8607a373a97 draft head 4
  | |
  o |  8:0da92be05148 draft head 3
  |/
  | o  7:4c35cdf97d5e draft head 2 commit 2
  | |
  | o  6:931820154288 draft head 2 commit 1
  |/
  | o  5:8cdc02b9bc63 draft head 1 draft 2
  | |
  | o  4:463b8c0d2973 draft head 1 draft 1
  | |
  | o  3:23a0c4eefcbf public head 1 public
  | |
  o |  2:4117331c3abb draft draft after public
  |/
  o  1:4426d359ea59 public public 1
  |
  o  0:54136a8ddf32 public root
  

Default base revision should stop at public changesets

  $ hg -q up 8cdc02b9bc63
  $ hg histedit --commands - <<EOF
  > pick 463b8c0d2973
  > pick 8cdc02b9bc63
  > EOF

Default base revision should stop at branchpoint

  $ hg -q up 4c35cdf97d5e
  $ hg histedit --commands - <<EOF
  > pick 931820154288
  > pick 4c35cdf97d5e
  > EOF

Default base revision should stop at merge commit

  $ hg -q up 8cde254db839
  $ hg histedit --commands - <<EOF
  > pick 6f2f0241f119
  > pick 8cde254db839
  > EOF

commit --amend should abort if histedit is in progress
(issue4800) and markers are not being created.
Eventually, histedit could perhaps look at `source` extra,
in which case this test should be revisited.

  $ hg -q up 8cde254db839
  $ hg histedit 6f2f0241f119 --commands - <<EOF
  > pick 8cde254db839
  > edit 6f2f0241f119
  > EOF
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging foo
  warning: conflicts while merging foo! (edit, then use 'hg resolve --mark')
  Fix up the change (pick 8cde254db839)
  (hg histedit --continue to resume)
  [1]
  $ hg resolve -m --all
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg histedit --cont
  merging foo
  warning: conflicts while merging foo! (edit, then use 'hg resolve --mark')
  Editing (6f2f0241f119), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]
  $ hg resolve -m --all
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg commit --amend -m 'reject this fold'
  abort: histedit in progress
  (use 'hg histedit --continue' or 'hg histedit --abort')
  [255]

With markers enabled, histedit does not get confused, and
amend should not be blocked by the ongoing histedit.

  $ cat >>$HGRCPATH <<EOF
  > [experimental]
  > evolution=createmarkers,allowunstable
  > EOF
  $ hg commit --amend -m 'allow this fold'
  $ hg histedit --continue

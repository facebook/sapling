
#require no-eden


Test argument handling and various data parsing
==================================================

  $ enable histedit

Repo setup.
  $ sl init foo
  $ cd foo
  $ echo alpha >> alpha
  $ sl addr
  adding alpha
  $ sl ci -m one
  $ echo alpha >> alpha
  $ sl ci -m two
  $ echo alpha >> alpha
  $ sl ci -m three
  $ echo alpha >> alpha
  $ sl ci -m four
  $ echo alpha >> alpha
  $ sl ci -m five

  $ sl log --style compact --graph
  @     08d98a8350f3   1970-01-01 00:00 +0000   test
  │    five
  │
  o     c8e68270e35a   1970-01-01 00:00 +0000   test
  │    four
  │
  o     eb57da33312f   1970-01-01 00:00 +0000   test
  │    three
  │
  o     579e40513370   1970-01-01 00:00 +0000   test
  │    two
  │
  o     6058cbb6cfd7   1970-01-01 00:00 +0000   test
       one
  

histedit --continue/--abort with no existing state
--------------------------------------------------

  $ sl histedit --continue
  abort: no histedit in progress
  [255]
  $ sl histedit --abort
  abort: no histedit in progress
  [255]

Run a dummy edit to make sure we get tip^^ correctly via revsingle.
--------------------------------------------------------------------

  $ HGEDITOR=cat sl histedit "tip^^"
  pick eb57da33312f three
  pick c8e68270e35a four
  pick 08d98a8350f3 five
  
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
  #  b, base = checkout changeset and apply further changesets from there
  #  d, drop = remove commit from history
  #  f, fold = use commit, but combine it with the one above
  #  i, into = like fold, but keep only this commit's description
  #  r, roll = like fold, but discard this commit's description and date
  #

Run on a revision not ancestors of the current working directory.
--------------------------------------------------------------------

  $ sl up 'desc(three)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl histedit -r 'desc(five)'
  abort: 08d98a8350f3 is not an ancestor of working directory
  [255]
  $ sl up --quiet tip


Test that we pick the minimum of a revrange
---------------------------------------

  $ HGEDITOR=cat sl histedit 'desc(three)::' --commands - << EOF
  > pick eb57da33312f 2 three
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  $ sl up --quiet tip

  $ HGEDITOR=cat sl histedit 'tip:desc(three)' --commands - << EOF
  > pick eb57da33312f 2 three
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  $ sl up --quiet tip

Test config specified default
-----------------------------

  $ HGEDITOR=cat sl histedit --config "histedit.defaultrev=only(.) - ::eb57da33312f" --commands - << EOF
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF

Run on a revision not descendants of the initial parent
--------------------------------------------------------------------

Test the message shown for inconsistent histedit state, which may be
created (and forgotten) by Mercurial earlier than 2.7. This emulates
Mercurial earlier than 2.7 by renaming ".sl/histedit-state"
temporarily.

  $ sl log -G -T '{shortest(node)} {desc}\n' -r 'desc(three)'::
  @  08d9 five
  │
  o  c8e6 four
  │
  o  eb57 three
  │
  ~
  $ HGEDITOR=cat sl histedit -r 'desc(five)' --commands - << EOF
  > edit 08d98a8350f3 4 five
  > EOF
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  reverting alpha
  Editing (08d98a8350f3), you may commit or record as needed now.
  (sl histedit --continue to resume)
  [1]

  $ sl graft --continue
  abort: no graft in progress
  (continue: sl histedit --continue)
  [255]

  $ mv .sl/histedit-state .sl/histedit-state.back
  $ sl goto --quiet --clean 'desc(three)'
  $ echo alpha >> alpha
  $ mv .sl/histedit-state.back .sl/histedit-state

  $ sl histedit --continue
  $ sl log -G -T '{shortest(node)} {desc}\n' -r 'desc(three)'::
  @  f5ed five
  │
  │ o  c8e6 four
  ├─╯
  o  eb57 three
  │
  ~

  $ sl debugstrip -q -r f5ed
  $ sl up -q 08d98a8350f3

Test that missing revisions are detected
---------------------------------------

  $ HGEDITOR=cat sl histedit "tip^^" --commands - << EOF
  > pick eb57da33312f 2 three
  > pick 08d98a8350f3 4 five
  > EOF
  sl: parse error: missing rules for changeset c8e68270e35a
  (use "drop c8e68270e35a" to discard, see also: 'sl help -e histedit.config')
  [255]

Test that extra revisions are detected
---------------------------------------

  $ HGEDITOR=cat sl histedit "tip^^" --commands - << EOF
  > pick 6058cbb6cfd7 0 one
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  sl: parse error: pick "6058cbb6cfd7" changeset was not a candidate
  (only use listed changesets)
  [255]

Test malformed line
---------------------------------------

  $ HGEDITOR=cat sl histedit "tip^^" --commands - << EOF
  > pickeb57da33312f2three
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  sl: parse error: malformed line "pickeb57da33312f2three"
  [255]

Test unknown changeset
---------------------------------------

  $ HGEDITOR=cat sl histedit "tip^^" --commands - << EOF
  > pick 0123456789ab 2 three
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  sl: parse error: unknown changeset 0123456789ab listed
  [255]

Test unknown command
---------------------------------------

  $ HGEDITOR=cat sl histedit "tip^^" --commands - << EOF
  > coin eb57da33312f 2 three
  > pick c8e68270e35a 3 four
  > pick 08d98a8350f3 4 five
  > EOF
  sl: parse error: unknown action "coin"
  [255]

Test duplicated changeset
---------------------------------------

So one is missing and one appear twice.

  $ HGEDITOR=cat sl histedit "tip^^" --commands - << EOF
  > pick eb57da33312f 2 three
  > pick eb57da33312f 2 three
  > pick 08d98a8350f3 4 five
  > EOF
  sl: parse error: duplicated command for changeset eb57da33312f
  [255]

Test bogus rev
---------------------------------------

  $ HGEDITOR=cat sl histedit "tip^^" --commands - << EOF
  > pick eb57da33312f 2 three
  > pick 0
  > pick 08d98a8350f3 4 five
  > EOF
  sl: parse error: invalid changeset 0
  [255]

Test short version of command
---------------------------------------

Note: we use varying amounts of white space between command name and changeset
short hash. This tests issue3893.

  $ HGEDITOR=cat sl histedit "tip^^" --commands - << EOF
  > pick eb57da33312f 2 three
  > p    c8e68270e35a 3 four
  > f 08d98a8350f3 4 five
  > EOF
  four
  ***
  five
  
  
  
  SL: Enter commit message.  Lines beginning with 'SL:' are removed.
  SL: Leave message empty to abort commit.
  SL: --
  SL: user: test
  SL: changed alpha
  folded c8e68270e35a, 08d98a8350f3 -> 2a30f3cfee78 "four"

  $ sl goto -q 'desc(three)'
  $ echo x > x
  $ sl add x
  $ sl commit -m'x' x
  $ sl histedit -r 'heads(all())'
  abort: The specified revisions must have exactly one common root
  [255]

Test that trimming description using multi-byte characters
--------------------------------------------------------------------

  $ $PYTHON <<EOF
  > fp = open('logfile', 'wb')
  > fp.write(b'12345678901234567890123456789012345678901234567890' +
  >          b'12345') # there are 5 more columns for 80 columns
  > 
  > # 2 x 4 = 8 columns, but 3 x 4 = 12 bytes
  > fp.write(b'\xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88')
  > 
  > fp.close()
  > EOF
  $ echo xx >> x
  $ sl --encoding utf-8 commit --logfile logfile

  $ HGEDITOR=cat sl --encoding utf-8 histedit tip
  pick 3d3ea1f3a10b 1234567890123456789012345678901234567890123456789012345\xe3\x81\x82\xe3\x81\x84... (esc)
  
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
  #  b, base = checkout changeset and apply further changesets from there
  #  d, drop = remove commit from history
  #  f, fold = use commit, but combine it with the one above
  #  i, into = like fold, but keep only this commit's description
  #  r, roll = like fold, but discard this commit's description and date
  #

Test --continue with --keep

  $ sl debugstrip -q -r .
  $ sl histedit '.^' -q --keep --commands - << EOF
  > edit eb57da33312f 2 three
  > pick f3cfcca30c44 4 x
  > EOF
  Editing (eb57da33312f), you may commit or record as needed now.
  (sl histedit --continue to resume)
  [1]
  $ echo edit >> alpha
  $ sl histedit -q --continue
  $ sl log -G -T '{node|short} {desc}'
  @  8fda0c726bf2 x
  │
  o  63379946892c three
  │
  │ x  f3cfcca30c44 x
  │ │
  │ │ o  2a30f3cfee78 four
  │ ├─╯  ***
  │ │    five
  │ x  eb57da33312f three
  ├─╯
  o  579e40513370 two
  │
  o  6058cbb6cfd7 one
  

Test that abort fails gracefully on exception
----------------------------------------------
  $ sl histedit . -q --commands - << EOF
  > edit 8fda0c726bf2 6 x
  > EOF
  Editing (8fda0c726bf2), you may commit or record as needed now.
  (sl histedit --continue to resume)
  [1]
Corrupt histedit state file
  $ sed 's/8fda0c726bf2/123456789012/' .sl/histedit-state > ../corrupt-histedit
  $ mv ../corrupt-histedit .sl/histedit-state
  $ sl histedit --abort
  warning: encountered an exception during histedit --abort; the repository may not have been completely cleaned up
  abort: * (glob)
  [255]
Histedit state has been exited
  $ sl summary -q
  parent: 63379946892c 
  commit: 1 added, 1 unknown

  $ cd ..

Set up default base revision tests

  $ sl init defaultbase
  $ cd defaultbase
  $ touch foo
  $ sl -q commit -A -m root
  $ echo 1 > foo
  $ sl commit -m 'public 1'
  $ sl debugmakepublic -r .
  $ echo 2 > foo
  $ sl commit -m 'draft after public'
  $ sl -q up -r 4426d359ea5987d8bcbece7ca93bb09083b857cd
  $ echo 3 > foo
  $ sl commit -m 'head 1 public'
  $ sl debugmakepublic -r .
  $ echo 4 > foo
  $ sl commit -m 'head 1 draft 1'
  $ echo 5 > foo
  $ sl commit -m 'head 1 draft 2'
  $ sl -q up -r 4117331c3abbf74a1c68983ceac01dfa82cfe085
  $ echo 6 > foo
  $ sl commit -m 'head 2 commit 1'
  $ echo 7 > foo
  $ sl commit -m 'head 2 commit 2'
  $ sl -q up -r 4117331c3abbf74a1c68983ceac01dfa82cfe085
  $ echo 8 > foo
  $ sl commit -m 'head 3'
  $ sl -q up -r 4117331c3abbf74a1c68983ceac01dfa82cfe085
  $ echo 9 > foo
  $ sl commit -m 'head 4'
  $ sl merge --tool :local -r 0da92be051485df39d1feb5bbb7cad588040a23e
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ sl commit -m 'merge head 3 into head 4'
  $ echo 11 > foo
  $ sl commit -m 'commit 1 after merge'
  $ echo 12 > foo
  $ sl commit -m 'commit 2 after merge'

  $ sl log -G -T '{node|short} {phase} {desc}\n'
  @  8cde254db839 draft commit 2 after merge
  │
  o  6f2f0241f119 draft commit 1 after merge
  │
  o    90506cc76b00 draft merge head 3 into head 4
  ├─╮
  │ o  f8607a373a97 draft head 4
  │ │
  o │  0da92be05148 draft head 3
  ├─╯
  │ o  4c35cdf97d5e draft head 2 commit 2
  │ │
  │ o  931820154288 draft head 2 commit 1
  ├─╯
  │ o  8cdc02b9bc63 draft head 1 draft 2
  │ │
  │ o  463b8c0d2973 draft head 1 draft 1
  │ │
  │ o  23a0c4eefcbf public head 1 public
  │ │
  o │  4117331c3abb draft draft after public
  ├─╯
  o  4426d359ea59 public public 1
  │
  o  54136a8ddf32 public root
  

Default base revision should stop at public changesets

  $ sl -q up 8cdc02b9bc63
  $ sl histedit --commands - <<EOF
  > pick 463b8c0d2973
  > pick 8cdc02b9bc63
  > EOF

Default base revision should stop at branchpoint

  $ sl -q up 4c35cdf97d5e
  $ sl histedit --commands - <<EOF
  > pick 931820154288
  > pick 4c35cdf97d5e
  > EOF

Default base revision should stop at merge commit

  $ sl -q up 8cde254db839
  $ sl histedit --commands - <<EOF
  > pick 6f2f0241f119
  > pick 8cde254db839
  > EOF

commit --amend during histedit is okay.

  $ sl -q up 8cde254db839
  $ sl histedit 6f2f0241f119 --commands - <<EOF
  > pick 8cde254db839
  > edit 6f2f0241f119
  > EOF
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging foo
  warning: 1 conflicts while merging foo! (edit, then use 'sl resolve --mark')
  Fix up the change (pick 8cde254db839)
  (sl histedit --continue to resume)
  [1]
  $ sl resolve -m --all
  (no more unresolved files)
  continue: sl histedit --continue
  $ sl histedit --cont
  merging foo
  warning: 1 conflicts while merging foo! (edit, then use 'sl resolve --mark')
  Editing (6f2f0241f119), you may commit or record as needed now.
  (sl histedit --continue to resume)
  [1]
  $ sl resolve -m --all
  (no more unresolved files)
  continue: sl histedit --continue

  $ sl commit --amend -m 'allow this fold'
  $ sl histedit --continue
  folded 6f2f0241f119, 8cde254db839 -> c2f267b6fd98 "allow this fold"

  $ cd ..

Test autoverb feature

  $ sl init autoverb
  $ cd autoverb
  $ echo alpha >> alpha
  $ sl ci -qAm one
  $ echo alpha >> alpha
  $ sl ci -qm two
  $ echo beta >> beta
  $ sl ci -qAm "roll! one"

  $ sl log --style compact --graph
  @     4f34d0f8b5fa   1970-01-01 00:00 +0000   test
  │    roll! one
  │
  o     579e40513370   1970-01-01 00:00 +0000   test
  │    two
  │
  o     6058cbb6cfd7   1970-01-01 00:00 +0000   test
       one
  

Check that 'roll' is selected by default

  $ HGEDITOR=cat sl histedit 6058cbb6cfd78cfdef42aa56faa272ee45d4b7dc --config experimental.histedit.autoverb=True
  pick 6058cbb6cfd7 one
  roll 4f34d0f8b5fa roll! one
  pick 579e40513370 two
  
  # Edit history between 6058cbb6cfd7 and 4f34d0f8b5fa
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
  #  b, base = checkout changeset and apply further changesets from there
  #  d, drop = remove commit from history
  #  f, fold = use commit, but combine it with the one above
  #  i, into = like fold, but keep only this commit's description
  #  r, roll = like fold, but discard this commit's description and date
  #
  folded 6058cbb6cfd7, 4f34d0f8b5fa -> d6de9f4a2eca "one"

  $ cd ..

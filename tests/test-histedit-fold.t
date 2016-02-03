Test histedit extension: Fold commands
======================================

This test file is dedicated to testing the fold command in non conflicting
case.

Initialization
---------------


  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [alias]
  > logt = log --template '{rev}:{node|short} {desc|firstline}\n'
  > [extensions]
  > histedit=
  > EOF


Simple folding
--------------------
  $ initrepo ()
  > {
  >     hg init r
  >     cd r
  >     for x in a b c d e f ; do
  >         echo $x > $x
  >         hg add $x
  >         hg ci -m $x
  >     done
  > }

  $ initrepo

log before edit
  $ hg logt --graph
  @  5:652413bf663e f
  |
  o  4:e860deea161a e
  |
  o  3:055a42cdd887 d
  |
  o  2:177f92b77385 c
  |
  o  1:d2ae7f538514 b
  |
  o  0:cb9a9f314b8b a
  

  $ hg histedit 177f92b77385 --commands - 2>&1 <<EOF | fixbundle
  > pick e860deea161a e
  > pick 652413bf663e f
  > fold 177f92b77385 c
  > pick 055a42cdd887 d
  > EOF

log after edit
  $ hg logt --graph
  @  4:9c277da72c9b d
  |
  o  3:6de59d13424a f
  |
  o  2:ee283cb5f2d5 e
  |
  o  1:d2ae7f538514 b
  |
  o  0:cb9a9f314b8b a
  

post-fold manifest
  $ hg manifest
  a
  b
  c
  d
  e
  f


check histedit_source

  $ hg log --debug --rev 3
  changeset:   3:6de59d13424a8a13acd3e975514aed29dd0d9b2d
  phase:       draft
  parent:      2:ee283cb5f2d5955443f23a27b697a04339e9a39a
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    3:81eede616954057198ead0b2c73b41d1f392829a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      c f
  extra:       branch=default
  extra:       histedit_source=a4f7421b80f79fcc59fff01bcbf4a53d127dd6d3,177f92b773850b59254aa5e923436f921b55483b
  description:
  f
  ***
  c
  
  

rollup will fold without preserving the folded commit's message

  $ OLDHGEDITOR=$HGEDITOR
  $ HGEDITOR=false
  $ hg histedit d2ae7f538514 --commands - 2>&1 <<EOF | fixbundle
  > pick d2ae7f538514 b
  > roll ee283cb5f2d5 e
  > pick 6de59d13424a f
  > pick 9c277da72c9b d
  > EOF

  $ HGEDITOR=$OLDHGEDITOR

log after edit
  $ hg logt --graph
  @  3:c4a9eb7989fc d
  |
  o  2:8e03a72b6f83 f
  |
  o  1:391ee782c689 b
  |
  o  0:cb9a9f314b8b a
  

description is taken from rollup target commit

  $ hg log --debug --rev 1
  changeset:   1:391ee782c68930be438ccf4c6a403daedbfbffa5
  phase:       draft
  parent:      0:cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    1:b5e112a3a8354e269b1524729f0918662d847c38
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      b e
  extra:       branch=default
  extra:       histedit_source=d2ae7f538514cd87c17547b0de4cea71fe1af9fb,ee283cb5f2d5955443f23a27b697a04339e9a39a
  description:
  b
  
  

check saving last-message.txt

  $ cat > $TESTTMP/abortfolding.py <<EOF
  > from mercurial import util
  > def abortfolding(ui, repo, hooktype, **kwargs):
  >     ctx = repo[kwargs.get('node')]
  >     if set(ctx.files()) == set(['c', 'd', 'f']):
  >         return True # abort folding commit only
  >     ui.warn('allow non-folding commit\\n')
  > EOF
  $ cat > .hg/hgrc <<EOF
  > [hooks]
  > pretxncommit.abortfolding = python:$TESTTMP/abortfolding.py:abortfolding
  > EOF

  $ cat > $TESTTMP/editor.sh << EOF
  > echo "==== before editing"
  > cat \$1
  > echo "===="
  > echo "check saving last-message.txt" >> \$1
  > EOF

  $ rm -f .hg/last-message.txt
  $ hg status --rev '8e03a72b6f83^1::c4a9eb7989fc'
  A c
  A d
  A f
  $ HGEDITOR="sh $TESTTMP/editor.sh" hg histedit 8e03a72b6f83 --commands - 2>&1 <<EOF
  > pick 8e03a72b6f83 f
  > fold c4a9eb7989fc d
  > EOF
  allow non-folding commit
  ==== before editing
  f
  ***
  c
  ***
  d
  
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added c
  HG: added d
  HG: added f
  ====
  transaction abort!
  rollback completed
  abort: pretxncommit.abortfolding hook failed
  [255]

  $ cat .hg/last-message.txt
  f
  ***
  c
  ***
  d
  
  
  
  check saving last-message.txt

  $ cd ..
  $ rm -r r

folding preserves initial author
--------------------------------

  $ initrepo

  $ hg ci --user "someone else" --amend --quiet

tip before edit
  $ hg log --rev .
  changeset:   5:a00ad806cb55
  tag:         tip
  user:        someone else
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  

  $ hg --config progress.debug=1 --debug \
  > histedit e860deea161a --commands - 2>&1 <<EOF | \
  > egrep 'editing|unresolved'
  > pick e860deea161a e
  > fold a00ad806cb55 f
  > EOF
  editing: pick e860deea161a 4 e 1/2 changes (50.00%)
  editing: fold a00ad806cb55 5 f 2/2 changes (100.00%)

tip after edit
  $ hg log --rev .
  changeset:   4:698d4e8040a1
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  

  $ cd ..
  $ rm -r r

folding and creating no new change doesn't break:
-------------------------------------------------

folded content is dropped during a merge. The folded commit should properly disappear.

  $ mkdir fold-to-empty-test
  $ cd fold-to-empty-test
  $ hg init
  $ printf "1\n2\n3\n" > file
  $ hg add file
  $ hg commit -m '1+2+3'
  $ echo 4 >> file
  $ hg commit -m '+4'
  $ echo 5 >> file
  $ hg commit -m '+5'
  $ echo 6 >> file
  $ hg commit -m '+6'
  $ hg logt --graph
  @  3:251d831eeec5 +6
  |
  o  2:888f9082bf99 +5
  |
  o  1:617f94f13c0f +4
  |
  o  0:0189ba417d34 1+2+3
  

  $ hg histedit 1 --commands - << EOF
  > pick 617f94f13c0f 1 +4
  > drop 888f9082bf99 2 +5
  > fold 251d831eeec5 3 +6
  > EOF
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging file
  warning: conflicts while merging file! (edit, then use 'hg resolve --mark')
  Fix up the change (fold 251d831eeec5)
  (hg histedit --continue to resume)
  [1]
There were conflicts, we keep P1 content. This
should effectively drop the changes from +6.
  $ hg status
  M file
  ? file.orig
  $ hg resolve -l
  U file
  $ hg revert -r 'p1()' file
  $ hg resolve --mark file
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg histedit --continue
  251d831eeec5: empty changeset
  saved backup bundle to $TESTTMP/*-backup.hg (glob)
  $ hg logt --graph
  @  1:617f94f13c0f +4
  |
  o  0:0189ba417d34 1+2+3
  

  $ cd ..


Test fold through dropped
-------------------------


Test corner case where folded revision is separated from its parent by a
dropped revision.


  $ hg init fold-with-dropped
  $ cd fold-with-dropped
  $ printf "1\n2\n3\n" > file
  $ hg commit -Am '1+2+3'
  adding file
  $ echo 4 >> file
  $ hg commit -m '+4'
  $ echo 5 >> file
  $ hg commit -m '+5'
  $ echo 6 >> file
  $ hg commit -m '+6'
  $ hg logt -G
  @  3:251d831eeec5 +6
  |
  o  2:888f9082bf99 +5
  |
  o  1:617f94f13c0f +4
  |
  o  0:0189ba417d34 1+2+3
  
  $ hg histedit 1 --commands -  << EOF
  > pick 617f94f13c0f 1 +4
  > drop 888f9082bf99 2 +5
  > fold 251d831eeec5 3 +6
  > EOF
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging file
  warning: conflicts while merging file! (edit, then use 'hg resolve --mark')
  Fix up the change (fold 251d831eeec5)
  (hg histedit --continue to resume)
  [1]
  $ cat > file << EOF
  > 1
  > 2
  > 3
  > 4
  > 5
  > EOF
  $ hg resolve --mark file
  (no more unresolved files)
  continue: hg histedit --continue
  $ hg commit -m '+5.2'
  created new head
  $ echo 6 >> file
  $ HGEDITOR=cat hg histedit --continue
  +4
  ***
  +5.2
  ***
  +6
  
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: changed file
  saved backup bundle to $TESTTMP/fold-with-dropped/.hg/strip-backup/55c8d8dc79ce-4066cd98-backup.hg (glob)
  saved backup bundle to $TESTTMP/fold-with-dropped/.hg/strip-backup/617f94f13c0f-a35700fc-backup.hg (glob)
  $ hg logt -G
  @  1:10c647b2cdd5 +4
  |
  o  0:0189ba417d34 1+2+3
  
  $ hg export tip
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 10c647b2cdd54db0603ecb99b2ff5ce66d5a5323
  # Parent  0189ba417d34df9dda55f88b637dcae9917b5964
  +4
  ***
  +5.2
  ***
  +6
  
  diff -r 0189ba417d34 -r 10c647b2cdd5 file
  --- a/file	Thu Jan 01 00:00:00 1970 +0000
  +++ b/file	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,3 +1,6 @@
   1
   2
   3
  +4
  +5
  +6
  $ cd ..


Folding with initial rename (issue3729)
---------------------------------------

  $ hg init fold-rename
  $ cd fold-rename
  $ echo a > a.txt
  $ hg add a.txt
  $ hg commit -m a
  $ hg rename a.txt b.txt
  $ hg commit -m rename
  $ echo b >> b.txt
  $ hg commit -m b

  $ hg logt --follow b.txt
  2:e0371e0426bc b
  1:1c4f440a8085 rename
  0:6c795aa153cb a

  $ hg histedit 1c4f440a8085 --commands - 2>&1 << EOF | fixbundle
  > pick 1c4f440a8085 rename
  > fold e0371e0426bc b
  > EOF

  $ hg logt --follow b.txt
  1:cf858d235c76 rename
  0:6c795aa153cb a

  $ cd ..

Folding with swapping
---------------------

This is an excuse to test hook with histedit temporary commit (issue4422)


  $ hg init issue4422
  $ cd issue4422
  $ echo a > a.txt
  $ hg add a.txt
  $ hg commit -m a
  $ echo b > b.txt
  $ hg add b.txt
  $ hg commit -m b
  $ echo c > c.txt
  $ hg add c.txt
  $ hg commit -m c

  $ hg logt
  2:a1a953ffb4b0 c
  1:199b6bb90248 b
  0:6c795aa153cb a

Setup the proper environment variable symbol for the platform, to be subbed
into the hook command.
#if windows
  $ NODE="%HG_NODE%"
#else
  $ NODE="\$HG_NODE"
#endif
  $ hg histedit 6c795aa153cb --config hooks.commit="echo commit $NODE" --commands - 2>&1 << EOF | fixbundle
  > pick 199b6bb90248 b
  > fold a1a953ffb4b0 c
  > pick 6c795aa153cb a
  > EOF
  commit 9599899f62c05f4377548c32bf1c9f1a39634b0c

  $ hg logt
  1:9599899f62c0 a
  0:79b99e9c8e49 b

  $ echo "foo" > amended.txt
  $ hg add amended.txt
  $ hg ci -q --config extensions.largefiles= --amend -I amended.txt

Test that folding multiple changes in a row doesn't show multiple
editors.

  $ echo foo >> foo
  $ hg add foo
  $ hg ci -m foo1
  $ echo foo >> foo
  $ hg ci -m foo2
  $ echo foo >> foo
  $ hg ci -m foo3
  $ hg logt
  4:21679ff7675c foo3
  3:b7389cc4d66e foo2
  2:0e01aeef5fa8 foo1
  1:578c7455730c a
  0:79b99e9c8e49 b
  $ cat > "$TESTTMP/editor.sh" <<EOF
  > echo ran editor >> "$TESTTMP/editorlog.txt"
  > cat \$1 >> "$TESTTMP/editorlog.txt"
  > echo END >> "$TESTTMP/editorlog.txt"
  > echo merged foos > \$1
  > EOF
  $ HGEDITOR="sh \"$TESTTMP/editor.sh\"" hg histedit 1 --commands - 2>&1 <<EOF | fixbundle
  > pick 578c7455730c 1 a
  > pick 0e01aeef5fa8 2 foo1
  > fold b7389cc4d66e 3 foo2
  > fold 21679ff7675c 4 foo3
  > EOF
  $ hg logt
  2:e8bedbda72c1 merged foos
  1:578c7455730c a
  0:79b99e9c8e49 b
Editor should have run only once
  $ cat $TESTTMP/editorlog.txt
  ran editor
  foo1
  ***
  foo2
  ***
  foo3
  
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added foo
  END

  $ cd ..

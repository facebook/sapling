Test histedit extention: Fold commands
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
  > graphlog=
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
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

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
  
  

  $ cd ..

folding and creating no new change doesn't break:
-------------------------------------------------

folded content is dropped during a merge. The folded commit should properly disapear.

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
  warning: conflicts during merge.
  merging file incomplete! (edit conflicts, then use 'hg resolve --mark')
  Fix up the change and run hg histedit --continue
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
  $ hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
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
  warning: conflicts during merge.
  merging file incomplete! (edit conflicts, then use 'hg resolve --mark')
  Fix up the change and run hg histedit --continue
  [1]
  $ cat > file << EOF
  > 1
  > 2
  > 3
  > 4
  > 5
  > EOF
  $ hg resolve --mark file
  $ hg commit -m '+5.2'
  created new head
  $ echo 6 >> file
  $ HGEDITOR=cat hg histedit --continue
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
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
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/fold-with-dropped/.hg/strip-backup/617f94f13c0f-backup.hg (glob)
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
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  reverting b.txt
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg logt --follow b.txt
  1:cf858d235c76 rename
  0:6c795aa153cb a

  $ cd ..

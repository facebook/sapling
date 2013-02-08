  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > histedit=
  > EOF

  $ EDITED="$TESTTMP/editedhistory"
  $ cat > $EDITED <<EOF
  > pick e860deea161a e
  > pick 652413bf663e f
  > fold 177f92b77385 c
  > pick 055a42cdd887 d
  > EOF
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
  $ hg log --graph
  @  changeset:   5:652413bf663e
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   4:e860deea161a
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   3:055a42cdd887
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   2:177f92b77385
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   1:d2ae7f538514
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  

edit the history
  $ HGEDITOR="cat \"$EDITED\" > " hg histedit 177f92b77385 2>&1 | fixbundle
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

log after edit
  $ hg log --graph
  @  changeset:   4:7e0a290363ed
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   3:5e24935bad3d
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     pick e860deea161a e
  |
  o  changeset:   2:ee283cb5f2d5
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   1:d2ae7f538514
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  

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
  changeset:   3:5e24935bad3d5a4486de3b90f233e991465ced72
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
  pick e860deea161a e
  pick 652413bf663e f
  fold 177f92b77385 c
  pick 055a42cdd887 d
  
  

  $ cd ..

folding and creating no new change doesn't break:
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
  $ hg log --graph
  @  changeset:   3:251d831eeec5
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     +6
  |
  o  changeset:   2:888f9082bf99
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     +5
  |
  o  changeset:   1:617f94f13c0f
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     +4
  |
  o  changeset:   0:0189ba417d34
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     1+2+3
  

  $ cat > editor.py <<EOF
  > import re, sys
  > rules = sys.argv[1]
  > data = open(rules).read()
  > data = re.sub(r'pick ([0-9a-f]{12} 2 \+5)', r'drop \1', data)
  > data = re.sub(r'pick ([0-9a-f]{12} 2 \+6)', r'fold \1', data)
  > open(rules, 'w').write(data)
  > EOF

  $ HGEDITOR='python editor.py' hg histedit 1
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
  ? editor.py
  ? file.orig
  $ hg resolve -l
  U file
  $ hg revert -r 'p1()' file
  $ hg resolve --mark file
  $ hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/*-backup.hg (glob)
  $ hg log --graph
  @  changeset:   1:617f94f13c0f
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     +4
  |
  o  changeset:   0:0189ba417d34
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     1+2+3
  

  $ cd ..

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
  $ hg log -G --template '{rev}:{node|short} {desc|firstline}\n'
  @  3:251d831eeec5 +6
  |
  o  2:888f9082bf99 +5
  |
  o  1:617f94f13c0f +4
  |
  o  0:0189ba417d34 1+2+3
  
  $ EDITED="$TESTTMP/editcommands"
  $ cat > $EDITED <<EOF
  > pick 617f94f13c0f 1 +4
  > drop 888f9082bf99 2 +5
  > fold 251d831eeec5 3 +6
  > EOF
  $ HGEDITOR="cat $EDITED >" hg histedit 1
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
  $ hg log -G
  @  changeset:   1:10c647b2cdd5
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     +4
  |
  o  changeset:   0:0189ba417d34
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     1+2+3
  
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


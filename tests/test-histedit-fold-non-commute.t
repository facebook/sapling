  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > histedit=
  > EOF

  $ EDITED="$TESTTMP/editedhistory"
  $ cat > $EDITED <<EOF
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > fold bfa474341cc9 does not commute with e
  > pick e860deea161a e
  > pick 652413bf663e f
  > EOF
  $ initrepo ()
  > {
  >     hg init $1
  >     cd $1
  >     for x in a b c d e f ; do
  >         echo $x > $x
  >         hg add $x
  >         hg ci -m $x
  >     done
  >     echo a >> e
  >     hg ci -m 'does not commute with e'
  >     cd ..
  > }

  $ initrepo r
  $ cd r

log before edit
  $ hg log --graph
  @  changeset:   6:bfa474341cc9
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     does not commute with e
  |
  o  changeset:   5:652413bf663e
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
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  1 out of 1 hunks FAILED -- saving rejects to file e.rej
  abort: Fix up the change and run hg histedit --continue

fix up
  $ echo a > e
  $ hg add e
  $ cat > cat.py <<EOF
  > import sys
  > print open(sys.argv[1]).read()
  > print
  > print
  > EOF
  $ HGEDITOR="python cat.py" hg histedit --continue 2>&1 | fixbundle | grep -v '2 files removed'
  d
  ***
  does not commute with e
  
  
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  file e already exists
  1 out of 1 hunks FAILED -- saving rejects to file e.rej
  abort: Fix up the change and run hg histedit --continue

just continue this time
  $ hg histedit --continue 2>&1 | fixbundle
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

log after edit
  $ hg log --graph
  @  changeset:   4:f768fd60ca34
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   3:671efe372e33
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
  

contents of e
  $ hg cat e
  a

manifest
  $ hg manifest
  a
  b
  c
  d
  e
  f

  $ cd ..

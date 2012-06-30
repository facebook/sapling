  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > histedit=
  > EOF

  $ EDITED=`pwd`/editedhistory
  $ cat > $EDITED <<EOF
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > edit e860deea161a e
  > pick 652413bf663e f
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
  $ HGEDITOR="cat $EDITED > " hg histedit 177f92b77385 2>&1 | fixbundle
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  abort: Make changes as needed, you may commit or record as needed now.
  When you are finished, run hg histedit --continue to resume.

commit, then edit the revision
  $ hg ci -m 'wat'
  created new head
  $ echo a > e
  $ HGEDITOR='echo "foobaz" > ' hg histedit --continue 2>&1 | fixbundle
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log --graph
  @  changeset:   6:bf757c081cd0
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   5:d6b15fed32d4
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     foobaz
  |
  o  changeset:   4:1a60820cd1f6
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     wat
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
  

  $ hg cat e
  a

  $ cat > $EDITED <<EOF
  > edit bf757c081cd0 f
  > EOF
  $ HGEDITOR="cat $EDITED > " hg histedit tip 2>&1 | fixbundle
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  abort: Make changes as needed, you may commit or record as needed now.
  When you are finished, run hg histedit --continue to resume.
  $ hg status
  A f
  $ HGEDITOR='true' hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status

log after edit
  $ hg log --limit 1
  changeset:   6:bf757c081cd0
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  

say we'll change the message, but don't.
  $ cat > ../edit.sh <<EOF
  > #!/bin/sh
  > cat \$1 | sed s/pick/mess/ > tmp
  > mv tmp \$1
  > EOF
  $ chmod +x ../edit.sh
  $ HGEDITOR="../edit.sh" hg histedit tip 2>&1 | fixbundle
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status
  $ hg log --limit 1
  changeset:   6:bf757c081cd0
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  

modify the message
  $ cat > $EDITED <<EOF
  > mess bf757c081cd0 f
  > EOF
  $ HGEDITOR="cat $EDITED > " hg histedit tip 2>&1 | fixbundle
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status
  $ hg log --limit 1
  changeset:   6:0b16746f8e89
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     mess bf757c081cd0 f
  

rollback should not work after a histedit
  $ hg rollback
  no rollback information available
  [1]

  $ cd ..

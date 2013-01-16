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
  $ HGEDITOR="cat \"$EDITED\" > " hg histedit 177f92b77385 2>&1 | fixbundle
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  abort: Make changes as needed, you may commit or record as needed now.
  When you are finished, run hg histedit --continue to resume.

Go at a random point and try to continue

  $ hg id -n
  3+
  $ hg up 0
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ HGEDITOR='echo foobaz > ' hg histedit --continue
  abort: working directory parent is not a descendant of 055a42cdd887
  (update to 055a42cdd887 or descendant and run "hg histedit --continue" again)
  [255]
  $ hg up 3
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

commit, then edit the revision
  $ hg ci -m 'wat'
  created new head
  $ echo a > e
  $ HGEDITOR='echo foobaz > ' hg histedit --continue 2>&1 | fixbundle
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log --graph
  @  changeset:   6:b5f70786f9b0
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   5:a5e1ba2f7afb
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

check histedit_source

  $ hg log --debug --rev 5
  changeset:   5:a5e1ba2f7afb899ef1581cea528fd885d2fca70d
  phase:       draft
  parent:      4:1a60820cd1f6004a362aa622ebc47d59bc48eb34
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    5:5ad3be8791f39117565557781f5464363b918a45
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       e
  extra:       branch=default
  extra:       histedit_source=e860deea161a2f77de56603b340ebbb4536308ae
  description:
  foobaz
  
  

  $ cat > $EDITED <<EOF
  > edit b5f70786f9b0 f
  > EOF
  $ HGEDITOR="cat \"$EDITED\" > " hg histedit tip 2>&1 | fixbundle
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  abort: Make changes as needed, you may commit or record as needed now.
  When you are finished, run hg histedit --continue to resume.
  $ hg status
  A f
  $ HGEDITOR='true' hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/b5f70786f9b0-backup.hg (glob)

  $ hg status

log after edit
  $ hg log --limit 1
  changeset:   6:a107ee126658
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  

say we'll change the message, but don't.
  $ cat > ../edit.sh <<EOF
  > cat "\$1" | sed s/pick/mess/ > tmp
  > mv tmp "\$1"
  > EOF
  $ HGEDITOR="sh ../edit.sh" hg histedit tip 2>&1 | fixbundle
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status
  $ hg log --limit 1
  changeset:   6:1fd3b2fe7754
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  

modify the message
  $ cat > $EDITED <<EOF
  > mess 1fd3b2fe7754 f
  > EOF
  $ HGEDITOR="cat \"$EDITED\" > " hg histedit tip 2>&1 | fixbundle
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status
  $ hg log --limit 1
  changeset:   6:5585e802ef99
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     mess 1fd3b2fe7754 f
  

rollback should not work after a histedit
  $ hg rollback
  no rollback information available
  [1]

  $ cd ..

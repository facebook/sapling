  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > histedit=
  > EOF

  $ echo "fbhistedit=$(echo $(dirname $TESTDIR))/fbhistedit.py" >> $HGRCPATH

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
  

stop & continue cannot preserve hashes without obsolence

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > stop e860deea161a e
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  Changes commited as 04d2fab98077. You may amend the commit now.
  When you are finished, run hg histedit --continue to resume

  $ hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/e860deea161a-fa46228d-backup.hg (glob)

  $ hg log --graph
  @  changeset:   5:794fe033d0a0
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   4:04d2fab98077
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
  

stop on a commit

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > stop 04d2fab98077 e
  > pick 794fe033d0a0 f
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  Changes commited as d28623a90f2b. You may amend the commit now.
  When you are finished, run hg histedit --continue to resume

  $ hg id -r . -i
  d28623a90f2b
  $ echo added > added
  $ hg add added
  $ hg commit --amend
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/d28623a90f2b-2e0800c1-amend-backup.hg (glob)

  $ hg log -v -r '.' --template '{files}\n'
  added e
  $ hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/04d2fab98077-3b7c8dde-backup.hg (glob)

  $ hg log --graph --template '{node|short} {desc} {files}\n'
  @  099559071076 f f
  |
  o  d51720eb7a13 e added e
  |
  o  055a42cdd887 d d
  |
  o  177f92b77385 c c
  |
  o  d2ae7f538514 b b
  |
  o  cb9a9f314b8b a a
  

check histedit_source

  $ hg log --debug --rev 4
  changeset:   4:d51720eb7a133e2dabf74a445e509a3900e9c0b5
  phase:       draft
  parent:      3:055a42cdd88768532f9cf79daa407fc8d138de9b
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    4:b2ebbc42649134e3236996c0a3b1c6ec526e8f2e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      added e
  extra:       amend_source=d28623a90f2b5c38b6c3ca503c86847b34c9bfdf
  extra:       branch=default
  extra:       histedit_source=04d2fab980779f332dec458cc944f28de8b43435
  description:
  e
  
  
fold a commit to check if other non-pick actions are handled correctly

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > fold 055a42cdd887 d
  > stop d51720eb7a13 e
  > pick 099559071076 f
  > EOF
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Changes commited as 08cf87522012. You may amend the commit now.
  When you are finished, run hg histedit --continue to resume

  $ hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/9377597fe60b-fe8de266-backup.hg (glob)
  saved backup bundle to $TESTTMP/r/.hg/strip-backup/177f92b77385-97654820-backup.hg (glob)

  $ hg log --graph --template '{node|short} {desc} {files}\n'
  @  3c9ba74168ea f f
  |
  o  08cf87522012 e added e
  |
  o  66584b8c84e1 c
  |  ***
  |  d c d
  o  d2ae7f538514 b b
  |
  o  cb9a9f314b8b a a
  

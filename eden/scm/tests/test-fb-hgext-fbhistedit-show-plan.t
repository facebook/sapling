  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > fbhistedit=
  > histedit=
  > rebase=
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
  

a failing command should drop us into the shell

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > pick e860deea161a e
  > exec exit 1
  > exec exit 2
  > pick 652413bf663e f
  > exec exit 3
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  Command 'exit 1' failed with exit status 1

show-plan should work

  $ hg histedit --show-plan
  histedit plan (call "histedit --continue/--retry" to resume it or "histedit --abort" to abort it):
      exec exit 1
      exec exit 2
      pick 652413bf663e 5 f
      exec exit 3

continue should work

  $ hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Command 'exit 2' failed with exit status 2
  [1]

show-plan after consecutive failed execs

  $ hg histedit --show-plan
  histedit plan (call "histedit --continue/--retry" to resume it or "histedit --abort" to abort it):
      exec exit 2
      pick 652413bf663e 5 f
      exec exit 3

continue after consecutive failed execs

  $ hg histedit --continue
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Command 'exit 3' failed with exit status 3
  [1]

show-plan after the last entry

  $ hg histedit --show-plan
  histedit plan (call "histedit --continue/--retry" to resume it or "histedit --abort" to abort it):
      exec exit 3

continue after the last entry

  $ hg histedit --continue

  $ hg log --template '{node|short} {desc}' --graph
  @  652413bf663e f
  |
  o  e860deea161a e
  |
  o  055a42cdd887 d
  |
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  

Set up a repo

  $ cat <<EOF >> $HGRCPATH
  > [ui]
  > interactive = true
  > [experimental]
  > crecord = true
  > crecordtest = testModeCommands
  > EOF

  $ hg init a
  $ cd a

Committing some changes but stopping on the way

  $ echo "a" > a
  $ hg add a
  $ cat <<EOF >testModeCommands
  > TOGGLE
  > X
  > EOF
  $ hg commit -i  -m "a" -d "0 0"
  no changes to record
  $ hg tip
  changeset:   -1:000000000000
  tag:         tip
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  

Committing some changes

  $ cat <<EOF >testModeCommands
  > X
  > EOF
  $ hg commit -i  -m "a" -d "0 0"
  $ hg tip
  changeset:   0:cb9a9f314b8b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
Committing only one file

  $ echo "a" >> a
  >>> open('b', 'wb').write("1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n")
  $ hg add b
  $ cat <<EOF >testModeCommands
  > TOGGLE
  > KEY_DOWN
  > X
  > EOF
  $ hg commit -i  -m "one file" -d "0 0"
  $ hg tip
  changeset:   1:fb2705a663ea
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     one file
  
  $ hg cat -r tip a
  a
  $ cat a
  a
  a

Committing only one hunk while aborting edition of hunk

- Untoggle all the hunks, go down to the second file
- unfold it
- go down to second hunk (1 for the first hunk, 1 for the first hunkline, 1 for the second hunk, 1 for the second hunklike)
- toggle the second hunk
- edit the hunk and quit the editor imediately with non-zero status
- commit

  $ printf "printf 'editor ran\n'; exit 1" > editor.sh
  $ echo "x" > c
  $ cat b >> c
  $ echo "y" >> c
  $ mv c b
  $ cat <<EOF >testModeCommands
  > A
  > KEY_DOWN
  > f
  > KEY_DOWN
  > KEY_DOWN
  > KEY_DOWN
  > KEY_DOWN
  > TOGGLE
  > e
  > X
  > EOF
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit -i  -m "one hunk" -d "0 0"
  editor ran
  $ rm editor.sh
  $ hg tip
  changeset:   2:7d10dfe755a8
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     one hunk
  
  $ hg cat -r tip b
  1
  2
  3
  4
  5
  6
  7
  8
  9
  10
  y
  $ cat b
  x
  1
  2
  3
  4
  5
  6
  7
  8
  9
  10
  y
  $ hg commit -m "other hunks"
  $ hg tip
  changeset:   3:a6735021574d
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     other hunks
  
  $ hg cat -r tip b
  x
  1
  2
  3
  4
  5
  6
  7
  8
  9
  10
  y

Newly added files can be selected with the curses interface

  $ hg update -C .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "hello" > x
  $ hg add x
  $ cat <<EOF >testModeCommands
  > TOGGLE
  > TOGGLE
  > X
  > EOF
  $ hg st
  A x
  ? testModeCommands
  $ hg commit -i  -m "newly added file" -d "0 0"
  $ hg st
  ? testModeCommands


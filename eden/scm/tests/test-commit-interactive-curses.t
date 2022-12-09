#chg-compatible
#debugruntest-compatible

  $ configure mutation-norecord
#require tic

Set up a repo

  $ cp $HGRCPATH $HGRCPATH.pretest
  $ cat <<EOF >> $HGRCPATH
  > [ui]
  > interactive = true
  > interface = curses
  > [experimental]
  > crecordtest = testModeCommands
  > EOF

Record with noeol at eof (issue5268)
  $ hg init noeol
  $ cd noeol
  $ printf '0' > a
  $ printf '0\n' > b
  $ hg ci -Aqm initial
  $ printf '1\n0' > a
  $ printf '1\n0\n' > b
  $ cat <<EOF >testModeCommands
  > c
  > EOF
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit  -i -m "add hunks" -d "0 0"
  $ cd ..

Normal repo
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
  [1]
  $ hg tip
  commit:      000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  

Committing some changes

  $ cat <<EOF >testModeCommands
  > X
  > EOF
  $ hg commit -i  -m "a" -d "0 0"
  $ hg tip
  commit:      cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
Check that commit -i works with no changes
  $ hg commit -i
  no changes to record
  [1]

Committing only one file

  $ echo "a" >> a
  >>> _ = open('b', 'w').write("1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n")
  $ hg add b
  $ cat <<EOF >testModeCommands
  > TOGGLE
  > KEY_DOWN
  > X
  > EOF
  $ hg commit -i  -m "one file" -d "0 0"
  $ hg tip
  commit:      fb2705a663ea
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
- toggle on and off the amend mode (to check that it toggles off)
- edit the hunk and quit the editor immediately with non-zero status
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
  > a
  > a
  > e
  > X
  > EOF
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit -i  -m "one hunk" -d "0 0"
  editor ran
  $ rm editor.sh
  $ hg tip
  commit:      7d10dfe755a8
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
  commit:      a6735021574d
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

  $ hg goto -C .
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

Amend option works
  $ echo "hello world" > x
  $ hg diff -c .
  diff -r a6735021574d -r 2b0e9be4d336 x
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +hello
  $ cat <<EOF >testModeCommands
  > a
  > X
  > EOF
  $ hg commit -i  -m "newly added file" -d "0 0"
  $ hg diff -c .
  diff -r a6735021574d -r c1d239d165ae x
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +hello world

Make file empty
  $ rm x && touch x
  $ cat <<EOF >testModeCommands
  > X
  > EOF
  $ hg ci -i -m emptify -d "0 0"
  $ hg goto -C '.^' -q

Editing a hunk puts you back on that hunk when done editing (issue5041)
To do that, we change two lines in a file, pretend to edit the second line,
exit, toggle the line selected at the end of the edit and commit.
The first line should be recorded if we were put on the second line at the end
of the edit.

  $ hg goto -C .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "foo" > x
  $ echo "hello world" >> x
  $ echo "bar" >> x
  $ cat <<EOF >testModeCommands
  > f
  > KEY_DOWN
  > KEY_DOWN
  > KEY_DOWN
  > KEY_DOWN
  > e
  > TOGGLE
  > X
  > EOF
  $ printf "printf 'editor ran\n'; exit 0" > editor.sh
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit  -i -m "edit hunk" -d "0 0" -q
  editor ran
  $ hg cat -r . x
  foo
  hello world

Testing the review option. The entire final filtered patch should show
up in the editor and be editable. We will unselect the second file and
the first hunk of the third file. During review, we will decide that
"lower" sounds better than "bottom", and the final commit should
reflect this edition.

  $ hg goto -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "top" > c
  $ cat x >> c
  $ echo "bottom" >> c
  $ mv c x
  $ echo "third a" >> a
  $ echo "we will unselect this" >> b

  $ cat > editor.sh <<EOF
  > cat "\$1"
  > cat "\$1" | sed s/bottom/lower/ > tmp
  > mv tmp "\$1"
  > EOF
  $ cat > testModeCommands <<EOF
  > KEY_DOWN
  > TOGGLE
  > KEY_DOWN
  > f
  > KEY_DOWN
  > TOGGLE
  > R
  > EOF

  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit  -i -m "review hunks" -d "0 0"
  # To remove '-' lines, make them ' ' lines (context).
  # To remove '+' lines, delete them.
  # Lines starting with # will be removed from the patch.
  #
  # If the patch applies cleanly, the edited patch will immediately
  # be finalised. If it does not apply cleanly, rejects files will be
  # generated. You can use those when you try again.
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,2 +1,3 @@
   a
   a
  +third a
  diff --git a/x b/x
  --- a/x
  +++ b/x
  @@ -1,2 +1,3 @@
   foo
   hello world
  +bottom

  $ hg cat -r . a
  a
  a
  third a

  $ hg cat -r . b
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

  $ hg cat -r . x
  foo
  hello world
  lower

Check spacemovesdown

  $ cat <<EOF >> $HGRCPATH
  > [experimental]
  > spacemovesdown = true
  > EOF
  $ cat <<EOF >testModeCommands
  > TOGGLE
  > TOGGLE
  > X
  > EOF
  $ hg status -q
  M b
  M x
  $ hg commit -i -m "nothing to commit?" -d "0 0"
  no changes to record
  [1]

Check ui.interface logic for the chunkselector

The default interface is text
  $ cp $HGRCPATH.pretest $HGRCPATH
  $ chunkselectorinterface() {
  > hg debugshell -- <<'EOF'
  > print(repo.ui.interface("chunkselector"))
  > EOF
  > }
  $ chunkselectorinterface
  text

If only the default is set, we'll use that for the feature, too
  $ cp $HGRCPATH.pretest $HGRCPATH
  $ cat <<EOF >> $HGRCPATH
  > [ui]
  > interface = curses
  > EOF
  $ chunkselectorinterface
  curses

It is possible to override the default interface with a feature specific
interface
  $ cp $HGRCPATH.pretest $HGRCPATH
  $ cat <<EOF >> $HGRCPATH
  > [ui]
  > interface = text
  > interface.chunkselector = curses
  > EOF

  $ chunkselectorinterface
  curses

  $ cp $HGRCPATH.pretest $HGRCPATH
  $ cat <<EOF >> $HGRCPATH
  > [ui]
  > interface = curses
  > interface.chunkselector = text
  > EOF

  $ chunkselectorinterface
  text

If a bad interface name is given, we use the default value (with a nice
error message to suggest that the configuration needs to be fixed)

  $ cp $HGRCPATH.pretest $HGRCPATH
  $ cat <<EOF >> $HGRCPATH
  > [ui]
  > interface = blah
  > EOF
  $ chunkselectorinterface
  invalid value for ui.interface: blah (using text)
  text

  $ cp $HGRCPATH.pretest $HGRCPATH
  $ cat <<EOF >> $HGRCPATH
  > [ui]
  > interface = curses
  > interface.chunkselector = blah
  > EOF
  $ chunkselectorinterface
  invalid value for ui.interface.chunkselector: blah (using curses)
  curses

  $ cp $HGRCPATH.pretest $HGRCPATH
  $ cat <<EOF >> $HGRCPATH
  > [ui]
  > interface = blah
  > interface.chunkselector = curses
  > EOF
  $ chunkselectorinterface
  invalid value for ui.interface: blah
  curses

  $ cp $HGRCPATH.pretest $HGRCPATH
  $ cat <<EOF >> $HGRCPATH
  > [ui]
  > interface = blah
  > interface.chunkselector = blah
  > EOF
  $ chunkselectorinterface
  invalid value for ui.interface: blah
  invalid value for ui.interface.chunkselector: blah (using text)
  text

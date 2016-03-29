# Construct the following history tree:
#
# @  5:e1bb631146ca  b1
# |
# o  4:a4fdb3b883c4 0:b608b9236435  b1
# |
# | o  3:4b57d2520816 1:44592833ba9f
# | |
# | | o  2:063f31070f65
# | |/
# | o  1:44592833ba9f
# |/
# o  0:b608b9236435

  $ mkdir b1
  $ cd b1
  $ hg init
  $ echo foo > foo
  $ echo zero > a
  $ hg init sub
  $ echo suba > sub/suba
  $ hg --cwd sub ci -Am addsuba
  adding suba
  $ echo 'sub = sub' > .hgsub
  $ hg ci -qAm0
  $ echo one > a ; hg ci -m1
  $ echo two > a ; hg ci -m2
  $ hg up -q 1
  $ echo three > a ; hg ci -qm3
  $ hg up -q 0
  $ hg branch -q b1
  $ echo four > a ; hg ci -qm4
  $ echo five > a ; hg ci -qm5

Initial repo state:

  $ hg log -G --template '{rev}:{node|short} {parents} {branches}\n'
  @  5:ff252e8273df  b1
  |
  o  4:d047485b3896 0:60829823a42a  b1
  |
  | o  3:6efa171f091b 1:0786582aa4b1
  | |
  | | o  2:bd10386d478c
  | |/
  | o  1:0786582aa4b1
  |/
  o  0:60829823a42a
  

Make sure update doesn't assume b1 is a repository if invoked from outside:

  $ cd ..
  $ hg update b1
  abort: no repository found in '$TESTTMP' (.hg not found)!
  [255]
  $ cd b1

Test helper functions:

  $ revtest () {
  >     msg=$1
  >     dirtyflag=$2   # 'clean', 'dirty' or 'dirtysub'
  >     startrev=$3
  >     targetrev=$4
  >     opt=$5
  >     hg up -qC $startrev
  >     test $dirtyflag = dirty && echo dirty > foo
  >     test $dirtyflag = dirtysub && echo dirty > sub/suba
  >     hg up $opt $targetrev
  >     hg parent --template 'parent={rev}\n'
  >     hg stat -S
  > }

  $ norevtest () {
  >     msg=$1
  >     dirtyflag=$2   # 'clean', 'dirty' or 'dirtysub'
  >     startrev=$3
  >     opt=$4
  >     hg up -qC $startrev
  >     test $dirtyflag = dirty && echo dirty > foo
  >     test $dirtyflag = dirtysub && echo dirty > sub/suba
  >     hg up $opt
  >     hg parent --template 'parent={rev}\n'
  >     hg stat -S
  > }

Test cases are documented in a table in the update function of merge.py.
Cases are run as shown in that table, row by row.

  $ norevtest 'none clean linear' clean 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  parent=5

  $ norevtest 'none clean same'   clean 2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 other heads for branch "default"
  parent=2


  $ revtest 'none clean linear' clean 1 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  parent=2

  $ revtest 'none clean same'   clean 2 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  parent=3

  $ revtest 'none clean cross'  clean 3 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  parent=4


  $ revtest 'none dirty linear' dirty 1 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  parent=2
  M foo

  $ revtest 'none dirtysub linear' dirtysub 1 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  parent=2
  M sub/suba

  $ revtest 'none dirty same'   dirty 2 3
  abort: uncommitted changes
  (commit or update --clean to discard changes)
  parent=2
  M foo

  $ revtest 'none dirtysub same'   dirtysub 2 3
  abort: uncommitted changes
  (commit or update --clean to discard changes)
  parent=2
  M sub/suba

  $ revtest 'none dirty cross'  dirty 3 4
  abort: uncommitted changes
  (commit or update --clean to discard changes)
  parent=3
  M foo

  $ norevtest 'none dirty cross'  dirty 2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 other heads for branch "default"
  parent=2
  M foo

  $ revtest 'none dirtysub cross'  dirtysub 3 4
  abort: uncommitted changes
  (commit or update --clean to discard changes)
  parent=3
  M sub/suba

  $ revtest '-C dirty linear'   dirty 1 2 -C
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  parent=2

  $ revtest '-c dirty linear'   dirty 1 2 -c
  abort: uncommitted changes
  parent=1
  M foo

  $ revtest '-c dirtysub linear'   dirtysub 1 2 -c
  abort: uncommitted changes in subrepository 'sub'
  parent=1
  M sub/suba

  $ norevtest '-c clean same'   clean 2 -c
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 other heads for branch "default"
  parent=2

  $ revtest '-cC dirty linear'  dirty 1 2 -cC
  abort: cannot specify both -c/--check and -C/--clean
  parent=1
  M foo

  $ cd ..

Test updating with closed head
---------------------------------------------------------------------

  $ hg clone -U -q b1 closed-heads
  $ cd closed-heads

Test updating if at least one non-closed branch head exists

if on the closed branch head:
- update to "."
- "updated to a closed branch head ...." message is displayed
- "N other heads for ...." message is displayed

  $ hg update -q -C 3
  $ hg commit --close-branch -m 6
  $ norevtest "on closed branch head" clean 6
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  no open descendant heads on branch "default", updating to a closed head
  (committing will reopen the head, use `hg heads .` to see 1 other heads)
  parent=6

if descendant non-closed branch head exists, and it is only one branch head:
- update to it, even if its revision is less than closed one
- "N other heads for ...." message isn't displayed

  $ norevtest "non-closed 2 should be chosen" clean 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  parent=2

if all descendant branch heads are closed, but there is another branch head:
- update to the tipmost descendant head
- "updated to a closed branch head ...." message is displayed
- "N other heads for ...." message is displayed

  $ norevtest "all descendant branch heads are closed" clean 3
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  no open descendant heads on branch "default", updating to a closed head
  (committing will reopen the head, use `hg heads .` to see 1 other heads)
  parent=6

Test updating if all branch heads are closed

if on the closed branch head:
- update to "."
- "updated to a closed branch head ...." message is displayed
- "all heads of branch ...." message is displayed

  $ hg update -q -C 2
  $ hg commit --close-branch -m 7
  $ norevtest "all heads of branch default are closed" clean 6
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  no open descendant heads on branch "default", updating to a closed head
  (committing will reopen branch "default")
  parent=6

if not on the closed branch head:
- update to the tipmost descendant (closed) head
- "updated to a closed branch head ...." message is displayed
- "all heads of branch ...." message is displayed

  $ norevtest "all heads of branch default are closed" clean 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  no open descendant heads on branch "default", updating to a closed head
  (committing will reopen branch "default")
  parent=7

  $ cd ..

Test updating if "default" branch doesn't exist and no revision is
checked out (= "default" is used as current branch)

  $ hg init no-default-branch
  $ cd no-default-branch

  $ hg branch foobar
  marked working directory as branch foobar
  (branches are permanent and global, did you want a bookmark?)
  $ echo a > a
  $ hg commit -m "#0" -A
  adding a
  $ echo 1 >> a
  $ hg commit -m "#1"
  $ hg update -q 0
  $ echo 3 >> a
  $ hg commit -m "#2"
  created new head
  $ hg commit --close-branch -m "#3"

if there is at least one non-closed branch head:
- update to the tipmost branch head

  $ norevtest "non-closed 1 should be chosen" clean null
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  parent=1

if all branch heads are closed
- update to "tip"
- "updated to a closed branch head ...." message is displayed
- "all heads for branch "XXXX" are closed" message is displayed

  $ hg update -q -C 1
  $ hg commit --close-branch -m "#4"

  $ norevtest "all branches are closed" clean null
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  no open descendant heads on branch "foobar", updating to a closed head
  (committing will reopen branch "foobar")
  parent=4

  $ cd ../b1

Test obsolescence behavior
---------------------------------------------------------------------

successors should be taken in account when checking head destination

  $ cat << EOF >> $HGRCPATH
  > [ui]
  > logtemplate={rev}:{node|short} {desc|firstline}
  > [experimental]
  > evolution=createmarkers
  > EOF

Test no-argument update to a successor of an obsoleted changeset

  $ hg log -G
  o  5:ff252e8273df 5
  |
  o  4:d047485b3896 4
  |
  | o  3:6efa171f091b 3
  | |
  | | o  2:bd10386d478c 2
  | |/
  | @  1:0786582aa4b1 1
  |/
  o  0:60829823a42a 0
  
  $ hg book bm -r 3
  $ hg status
  M foo

We add simple obsolescence marker between 3 and 4 (indirect successors)

  $ hg id --debug -i -r 3
  6efa171f091b00a3c35edc15d48c52a498929953
  $ hg id --debug -i -r 4
  d047485b3896813b2a624e86201983520f003206
  $ hg debugobsolete 6efa171f091b00a3c35edc15d48c52a498929953 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  $ hg debugobsolete aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa d047485b3896813b2a624e86201983520f003206

Test that 5 is detected as a valid destination from 3 and also accepts moving
the bookmark (issue4015)

  $ hg up --quiet --hidden 3
  $ hg up 5
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg book bm
  moving bookmark 'bm' forward from 6efa171f091b
  $ hg bookmarks
   * bm                        5:ff252e8273df

Test that 4 is detected as the no-argument destination from 3 and also moves
the bookmark with it
  $ hg up --quiet 0          # we should be able to update to 3 directly
  $ hg up --quiet --hidden 3 # but not implemented yet.
  $ hg book -f bm
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updating bookmark bm
  $ hg book
   * bm                        4:d047485b3896

Test that 5 is detected as a valid destination from 1
  $ hg up --quiet 0          # we should be able to update to 3 directly
  $ hg up --quiet --hidden 3 # but not implemented yet.
  $ hg up 5
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test that 5 is not detected as a valid destination from 2
  $ hg up --quiet 0
  $ hg up --quiet 2
  $ hg up 5
  abort: uncommitted changes
  (commit or update --clean to discard changes)
  [255]

Test that we don't crash when updating from a pruned changeset (i.e. has no
successors). Behavior should probably be that we update to the first
non-obsolete parent but that will be decided later.
  $ hg id --debug -r 2
  bd10386d478cd5a9faf2e604114c8e6da62d3889
  $ hg up --quiet 0
  $ hg up --quiet 2
  $ hg debugobsolete bd10386d478cd5a9faf2e604114c8e6da62d3889
  $ hg up
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test experimental revset support

  $ hg log -r '_destupdate()'
  2:bd10386d478c 2 (no-eol)

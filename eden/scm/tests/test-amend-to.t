#chg-compatible

Set up test environment.
  $ configure mutation-norecord
  $ enable amend directaccess histedit rebase
Test that amend --to option
  $ newrepo repo
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }
  $ mkcommit "ROOT"
  $ hg debugmakepublic "desc(ROOT)"
  $ mkcommit "SIDE"
  $ hg debugmakepublic "desc(SIDE)"
  $ hg update -r "desc(ROOT)"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit "A"
  $ mkcommit "B"
  $ mkcommit "C"
Test
----

  $ hg log -G -vp -T "{desc} {node|short}"
  @  C a8df460dbbfediff -r c473644ee0e9 -r a8df460dbbfe C
  |  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |  +++ b/C	Thu Jan 01 00:00:00 1970 +0000
  |  @@ -0,0 +1,1 @@
  |  +C
  |
  o  B c473644ee0e9diff -r 2a34000d3544 -r c473644ee0e9 B
  |  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |  +++ b/B	Thu Jan 01 00:00:00 1970 +0000
  |  @@ -0,0 +1,1 @@
  |  +B
  |
  o  A 2a34000d3544diff -r ea207398892e -r 2a34000d3544 A
  |  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |  +++ b/A	Thu Jan 01 00:00:00 1970 +0000
  |  @@ -0,0 +1,1 @@
  |  +A
  |
  | o  SIDE 3c489f4f07a6diff -r ea207398892e -r 3c489f4f07a6 SIDE
  |/   --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |    +++ b/SIDE	Thu Jan 01 00:00:00 1970 +0000
  |    @@ -0,0 +1,1 @@
  |    +SIDE
  |
  o  ROOT ea207398892ediff -r 000000000000 -r ea207398892e ROOT
     --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
     +++ b/ROOT	Thu Jan 01 00:00:00 1970 +0000
     @@ -0,0 +1,1 @@
     +ROOT
  
  $ cat > testFile << EOF
  > line1
  > line2
  > line3
  > EOF
  $ hg add testFile
  $ hg amend --to dorito
  abort: revision 'dorito' cannot be found
  [255]
  $ hg amend --to c473644
  $ hg amend --to 3c489f4f07a6
  abort: revision '3c489f4f07a6' is not a parent of the working copy
  [255]
  $ hg amend --to 'children(ea207398892e)'
  abort: 'children(ea207398892e)' refers to multiple changesets
  [255]
  $ hg amend --to 'min(children(ea207398892e))'
  abort: revision 'min(children(ea207398892e))' is not a parent of the working copy
  [255]
  $ hg log -G -vp -T "{desc} {node|short}"
  @  C 86de924a3b95diff -r ce91eb673f02 -r 86de924a3b95 C
  |  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |  +++ b/C	Thu Jan 01 00:00:00 1970 +0000
  |  @@ -0,0 +1,1 @@
  |  +C
  |
  o  B ce91eb673f02diff -r 2a34000d3544 -r ce91eb673f02 B
  |  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |  +++ b/B	Thu Jan 01 00:00:00 1970 +0000
  |  @@ -0,0 +1,1 @@
  |  +B
  |  diff -r 2a34000d3544 -r ce91eb673f02 testFile
  |  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |  +++ b/testFile	Thu Jan 01 00:00:00 1970 +0000
  |  @@ -0,0 +1,3 @@
  |  +line1
  |  +line2
  |  +line3
  |
  o  A 2a34000d3544diff -r ea207398892e -r 2a34000d3544 A
  |  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |  +++ b/A	Thu Jan 01 00:00:00 1970 +0000
  |  @@ -0,0 +1,1 @@
  |  +A
  |
  | o  SIDE 3c489f4f07a6diff -r ea207398892e -r 3c489f4f07a6 SIDE
  |/   --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |    +++ b/SIDE	Thu Jan 01 00:00:00 1970 +0000
  |    @@ -0,0 +1,1 @@
  |    +SIDE
  |
  o  ROOT ea207398892ediff -r 000000000000 -r ea207398892e ROOT
     --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
     +++ b/ROOT	Thu Jan 01 00:00:00 1970 +0000
     @@ -0,0 +1,1 @@
     +ROOT
  
  $ hg status
  $ echo "line4" >> testFile
  $ hg ci -m "line4"
  $ echo "line5" >> testFile
  $ hg amend --to ce91eb673f02
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  merging testFile
  warning: 1 conflicts while merging testFile! (edit, then use 'hg resolve --mark')
  amend --to encountered an issue - use hg histedit to continue or abortFix up the change (roll 8a18ce6b4d69)
  (hg histedit --continue to resume)
  [1]
  $ hg histedit --abort > /dev/null
  $ hg hide .  > /dev/null

Test amending to commit when side branch is present
  $ hg update -q ce91eb67
  $ mkcommit "SIDE2"
  $ hg update -q 75f11a3d
  $ echo EXTRA >> A
  $ hg amend --to 2a34000
  $ hg log -G -vp -T "{desc} {node|short}"
  @  line4 19b94b1dc3f8diff -r 66a8a892708f -r 19b94b1dc3f8 testFile
  |  --- a/testFile	Thu Jan 01 00:00:00 1970 +0000
  |  +++ b/testFile	Thu Jan 01 00:00:00 1970 +0000
  |  @@ -1,3 +1,4 @@
  |   line1
  |   line2
  |   line3
  |  +line4
  |
  o  C 66a8a892708fdiff -r 33635c2e36d9 -r 66a8a892708f C
  |  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |  +++ b/C	Thu Jan 01 00:00:00 1970 +0000
  |  @@ -0,0 +1,1 @@
  |  +C
  |
  o  B 33635c2e36d9diff -r 4cc5b3966b69 -r 33635c2e36d9 B
  |  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |  +++ b/B	Thu Jan 01 00:00:00 1970 +0000
  |  @@ -0,0 +1,1 @@
  |  +B
  |  diff -r 4cc5b3966b69 -r 33635c2e36d9 testFile
  |  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |  +++ b/testFile	Thu Jan 01 00:00:00 1970 +0000
  |  @@ -0,0 +1,3 @@
  |  +line1
  |  +line2
  |  +line3
  |
  o  A 4cc5b3966b69diff -r ea207398892e -r 4cc5b3966b69 A
  |  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |  +++ b/A	Thu Jan 01 00:00:00 1970 +0000
  |  @@ -0,0 +1,2 @@
  |  +A
  |  +EXTRA
  |
  | o  SIDE2 059ea8bdb89ediff -r ce91eb673f02 -r 059ea8bdb89e SIDE2
  | |  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  | |  +++ b/SIDE2	Thu Jan 01 00:00:00 1970 +0000
  | |  @@ -0,0 +1,1 @@
  | |  +SIDE2
  | |
  | x  B ce91eb673f02diff -r 2a34000d3544 -r ce91eb673f02 B
  | |  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  | |  +++ b/B	Thu Jan 01 00:00:00 1970 +0000
  | |  @@ -0,0 +1,1 @@
  | |  +B
  | |  diff -r 2a34000d3544 -r ce91eb673f02 testFile
  | |  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  | |  +++ b/testFile	Thu Jan 01 00:00:00 1970 +0000
  | |  @@ -0,0 +1,3 @@
  | |  +line1
  | |  +line2
  | |  +line3
  | |
  | x  A 2a34000d3544diff -r ea207398892e -r 2a34000d3544 A
  |/   --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |    +++ b/A	Thu Jan 01 00:00:00 1970 +0000
  |    @@ -0,0 +1,1 @@
  |    +A
  |
  | o  SIDE 3c489f4f07a6diff -r ea207398892e -r 3c489f4f07a6 SIDE
  |/   --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  |    +++ b/SIDE	Thu Jan 01 00:00:00 1970 +0000
  |    @@ -0,0 +1,1 @@
  |    +SIDE
  |
  o  ROOT ea207398892ediff -r 000000000000 -r ea207398892e ROOT
     --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
     +++ b/ROOT	Thu Jan 01 00:00:00 1970 +0000
     @@ -0,0 +1,1 @@
     +ROOT
  

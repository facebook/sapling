
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "largefiles =" >> $HGRCPATH

Create the repository outside $HOME since largefiles write to
$HOME/.cache/largefiles.

  $ hg init test
  $ cd test
  $ echo "root" > root
  $ hg add root
  $ hg commit -m "Root commit"

  $ echo "large" > foo
  $ hg add --large foo
  $ hg commit -m "Add foo as a largefile"

  $ hg update -r 0
  getting changed largefiles
  0 largefiles updated, 1 removed
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo "normal" > foo
  $ hg add foo
  $ hg commit -m "Add foo as normal file"
  created new head

Normal file in the working copy, keeping the normal version:

  $ echo "n" | hg merge --config ui.interactive=Yes
  remote turned local normal file foo into a largefile
  use (l)argefile or keep (n)ormal file? 0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  0 largefiles updated, 0 removed

  $ hg status
  $ cat foo
  normal

Normal file in the working copy, keeping the largefile version:

  $ hg update -q -C
  $ echo "l" | hg merge --config ui.interactive=Yes
  remote turned local normal file foo into a largefile
  use (l)argefile or keep (n)ormal file? 1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  1 largefiles updated, 0 removed

  $ hg status
  M foo

  $ hg diff --nodates
  diff -r fa129ab6b5a7 .hglf/foo
  --- /dev/null
  +++ b/.hglf/foo
  @@ -0,0 +1,1 @@
  +7f7097b041ccf68cc5561e9600da4655d21c6d18
  diff -r fa129ab6b5a7 foo
  --- a/foo
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -normal

  $ cat foo
  large

Largefile in the working copy, keeping the normal version:

  $ hg update -q -C -r 1
  $ echo "n" | hg merge --config ui.interactive=Yes
  remote turned local largefile foo into a normal file
  keep (l)argefile or use (n)ormal file? 1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  0 largefiles updated, 0 removed

  $ hg status
  M foo

  $ hg diff --nodates
  diff -r ff521236428a .hglf/foo
  --- a/.hglf/foo
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -7f7097b041ccf68cc5561e9600da4655d21c6d18
  diff -r ff521236428a foo
  --- /dev/null
  +++ b/foo
  @@ -0,0 +1,1 @@
  +normal

  $ cat foo
  normal

Largefile in the working copy, keeping the largefile version:

  $ hg update -q -C -r 1
  $ echo "l" | hg merge --config ui.interactive=Yes
  remote turned local largefile foo into a normal file
  keep (l)argefile or use (n)ormal file? 0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  1 largefiles updated, 0 removed

  $ hg status

  $ cat foo
  large

Whatever ... commit something so we can invoke merge when updating

  $ hg commit -m '3: Merge'

Updating from largefile to normal - no reason to prompt

  $ hg up -r 2
  getting changed largefiles
  0 largefiles updated, 0 removed
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat foo
  normal

(the update above used to leave the working dir in a very weird state - clean it
  $ hg up -qr null
  $ hg up -qr 2
)

Updating from normal to largefile - no reason to prompt

  $ hg up -r 3
  getting changed largefiles
  1 largefiles updated, 0 removed
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat foo
  large

  $ cd ..


Systematic testing of merges involving largefiles:

Ancestor: normal  Parent: normal=  Parent: large   result: large
Ancestor: normal  Parent: normal2  Parent: large   result: ?
Ancestor: large   Parent: large=   Parent: normal  result: normal
Ancestor: large   Parent: large2   Parent: normal  result: ?

All cases should try merging both ways.
"=" means same file content.

Prepare test repo:

  $ hg init merges
  $ cd merges
  $ touch f1
  $ hg ci -Aqm "0-root"

ancestor is "normal":
  $ echo normal > f
  $ hg ci -Aqm "1-normal-ancestor"
  $ touch f2
  $ hg ci -Aqm "2-normal-unchanged"
  $ hg tag -l "normal="
  $ echo normal2 > f
  $ hg ci -m "3-normal2"
  $ hg tag -l "normal2"
  $ hg up -qr 1
  $ hg rm f
  $ echo large > f
  $ hg add --large f
  $ hg ci -qm "4-normal-to-large"
  $ hg tag -l "large"

  $ hg up -qr null

ancestor is "large":
  $ echo large > f
  $ hg add --large f
  $ hg ci -qm "5-large-ancestor"
  $ touch f2
  $ hg ci -Aqm "6-large-unchanged"
  $ hg tag -l "large="
  $ echo large2 > f
  $ hg ci -m "7-large2"
  $ hg tag -l "large2"
  $ hg up -qr 5
  $ hg rm f
  $ echo normal > f
  $ hg ci -qAm "8-large-to-normal"
  $ hg tag -l "normal"

Ancestor: normal  Parent: normal=  Parent: large   result: large

  $ hg up -Cqr normal=
  $ hg merge -r large
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ cat f
  large

swap

  $ hg up -Cqr large
  $ hg merge -r normal=
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  0 largefiles updated, 0 removed
  $ cat f
  large

Ancestor: normal  Parent: normal2  Parent: large   result: ?
(annoying extra prompt ... but it do not do any serious harm)

  $ hg up -Cqr normal2
  $ hg merge -r large
  local changed f which remote deleted
  use (c)hanged version or (d)elete? c
  remote turned local normal file f into a largefile
  use (l)argefile or keep (n)ormal file? l
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ cat f
  large

  $ hg up -Cqr normal2
  $ ( echo c; echo n ) | hg merge -r large --config ui.interactive=Yes
  local changed f which remote deleted
  use (c)hanged version or (d)elete? remote turned local normal file f into a largefile
  use (l)argefile or keep (n)ormal file? 0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  0 largefiles updated, 0 removed
  $ cat f
  normal2

  $ hg up -Cqr normal2
  $ echo d | hg merge -r large --config ui.interactive=Yes
  local changed f which remote deleted
  use (c)hanged version or (d)elete? 1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ cat f
  large

swap

  $ hg up -Cqr large
  $ hg merge -r normal2
  remote changed f which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote turned local largefile f into a normal file
  keep (l)argefile or use (n)ormal file? l
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ cat f
  large

  $ hg up -Cqr large
  $ ( echo c; echo n ) | hg merge -r normal2 --config ui.interactive=Yes
  remote changed f which local deleted
  use (c)hanged version or leave (d)eleted? remote turned local largefile f into a normal file
  keep (l)argefile or use (n)ormal file? 2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  0 largefiles updated, 0 removed
  $ cat f
  normal2

  $ hg up -Cqr large
  $ echo d | hg merge -r normal2 --config ui.interactive=Yes
  remote changed f which local deleted
  use (c)hanged version or leave (d)eleted? 1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  0 largefiles updated, 0 removed
  $ cat f
  large

Ancestor: large   Parent: large=   Parent: normal  result: normal

  $ hg up -Cqr large=
  $ hg merge -r normal
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  0 largefiles updated, 0 removed
  $ cat f
  normal

swap

  $ hg up -Cqr normal
  $ hg merge -r large=
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  normal

Ancestor: large   Parent: large2   Parent: normal  result: ?
(annoying extra prompt ... but it do not do any serious harm)

  $ hg up -Cqr large2
  $ hg merge -r normal
  local changed .hglf/f which remote deleted
  use (c)hanged version or (d)elete? c
  remote turned local largefile f into a normal file
  keep (l)argefile or use (n)ormal file? l
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ cat f
  large2

  $ hg up -Cqr large2
  $ echo d | hg merge -r normal --config ui.interactive=Yes
  local changed .hglf/f which remote deleted
  use (c)hanged version or (d)elete? 1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  0 largefiles updated, 0 removed
  $ cat f
  normal

swap

  $ hg up -Cqr normal
  $ hg merge -r large2
  remote changed .hglf/f which local deleted
  use (c)hanged version or leave (d)eleted? c
  remote turned local normal file f into a largefile
  use (l)argefile or keep (n)ormal file? l
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ cat f
  large2

  $ hg up -Cqr normal
  $ echo d | hg merge -r large2 --config ui.interactive=Yes
  remote changed .hglf/f which local deleted
  use (c)hanged version or leave (d)eleted? 1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  normal

  $ cd ..

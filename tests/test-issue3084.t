
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
  foo has been turned into a largefile
  use (l)argefile or keep as (n)ormal file? 0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  0 largefiles updated, 0 removed

  $ hg status
  $ cat foo
  normal

Normal file in the working copy, keeping the largefile version:

  $ hg update -q -C
  $ echo "l" | hg merge --config ui.interactive=Yes
  foo has been turned into a largefile
  use (l)argefile or keep as (n)ormal file? 1 files updated, 0 files merged, 1 files removed, 0 files unresolved
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
  foo has been turned into a normal file
  keep as (l)argefile or use (n)ormal file? 1 files updated, 0 files merged, 1 files removed, 0 files unresolved
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
  foo has been turned into a normal file
  keep as (l)argefile or use (n)ormal file? 0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  getting changed largefiles
  1 largefiles updated, 0 removed

  $ hg status

  $ cat foo
  large

  $ cd ..

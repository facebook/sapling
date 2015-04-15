
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "largefiles =" >> $HGRCPATH

Create the repository outside $HOME since largefiles write to
$HOME/.cache/largefiles.

  $ hg init test
  $ cd test
  $ echo "root" > root
  $ hg add root
  $ hg commit -m "Root commit" --config extensions.largefiles=!

Ensure that .hg/largefiles isn't created before largefiles are added
#if unix-permissions
  $ chmod 555 .hg
#endif
  $ hg status
#if unix-permissions
  $ chmod 755 .hg
#endif

  $ test -f .hg/largefiles
  [1]

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
  use (l)argefile or keep (n)ormal file? n
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg status
  $ cat foo
  normal

Normal file in the working copy, keeping the largefile version:

  $ hg update -q -C
  $ echo "l" | hg merge --config ui.interactive=Yes
  remote turned local normal file foo into a largefile
  use (l)argefile or keep (n)ormal file? l
  getting changed largefiles
  1 largefiles updated, 0 removed
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

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
  keep (l)argefile or use (n)ormal file? n
  getting changed largefiles
  0 largefiles updated, 0 removed
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

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
  keep (l)argefile or use (n)ormal file? l
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

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

Ancestor: normal  Parent: normal-id  Parent: large   result: large
Ancestor: normal  Parent: normal2    Parent: large   result: ?
Ancestor: large   Parent: large-id   Parent: normal  result: normal
Ancestor: large   Parent: large2     Parent: normal  result: ?

All cases should try merging both ways.

Prepare test repo:

  $ hg init merges
  $ cd merges

prepare cases with "normal" ancestor:

  $ hg up -qr null
  $ echo normal > f
  $ hg ci -Aqm "normal-ancestor"
  $ hg tag -l "normal-ancestor"
  $ touch f2
  $ hg ci -Aqm "normal-id"
  $ hg tag -l "normal-id"
  $ echo normal2 > f
  $ hg ci -m "normal2"
  $ hg tag -l "normal2"
  $ echo normal > f
  $ hg ci -Aqm "normal-same"
  $ hg tag -l "normal-same"
  $ hg up -qr "normal-ancestor"
  $ hg rm f
  $ echo large > f
  $ hg add --large f
  $ hg ci -qm "large"
  $ hg tag -l "large"

prepare cases with "large" ancestor:

  $ hg up -qr null
  $ echo large > f
  $ hg add --large f
  $ hg ci -qm "large-ancestor"
  $ hg tag -l "large-ancestor"
  $ touch f2
  $ hg ci -Aqm "large-id"
  $ hg tag -l "large-id"
  $ echo large2 > f
  $ hg ci -m "large2"
  $ hg tag -l "large2"
  $ echo large > f
  $ hg ci -Aqm "large-same"
  $ hg tag -l "large-same"
  $ hg up -qr "large-ancestor"
  $ hg rm f
  $ echo normal > f
  $ hg ci -qAm "normal"
  $ hg tag -l "normal"

  $ hg log -GT '{tags}'
  @  normal tip
  |
  | o  large-same
  | |
  | o  large2
  | |
  | o  large-id
  |/
  o  large-ancestor
  
  o  large
  |
  | o  normal-same
  | |
  | o  normal2
  | |
  | o  normal-id
  |/
  o  normal-ancestor
  


Ancestor: normal  Parent: normal-id  Parent: large   result: large

  $ hg up -Cqr normal-id
  $ hg merge -r large
  getting changed largefiles
  1 largefiles updated, 0 removed
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  large

swap

  $ hg up -Cqr large
  $ hg merge -r normal-id
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  large

Ancestor: normal  Parent: normal-same  Parent: large   result: large

  $ hg up -Cqr normal-same
  $ hg merge -r large
  getting changed largefiles
  1 largefiles updated, 0 removed
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  large

swap

  $ hg up -Cqr large
  $ hg merge -r normal-same
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  large

Ancestor: normal  Parent: normal2  Parent: large   result: ?
(annoying extra prompt ... but it do not do any serious harm)

  $ hg up -Cqr normal2
  $ hg merge -r large
  remote turned local normal file f into a largefile
  use (l)argefile or keep (n)ormal file? l
  getting changed largefiles
  1 largefiles updated, 0 removed
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  large

  $ hg up -Cqr normal2
  $ echo n | hg merge -r large --config ui.interactive=Yes
  remote turned local normal file f into a largefile
  use (l)argefile or keep (n)ormal file? n
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  normal2

swap

  $ hg up -Cqr large
  $ hg merge -r normal2
  remote turned local largefile f into a normal file
  keep (l)argefile or use (n)ormal file? l
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  large

  $ hg up -Cqr large
  $ echo n | hg merge -r normal2 --config ui.interactive=Yes
  remote turned local largefile f into a normal file
  keep (l)argefile or use (n)ormal file? n
  getting changed largefiles
  0 largefiles updated, 0 removed
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  normal2

Ancestor: large   Parent: large-id   Parent: normal  result: normal

  $ hg up -Cqr large-id
  $ hg merge -r normal
  getting changed largefiles
  0 largefiles updated, 0 removed
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  normal

swap

  $ hg up -Cqr normal
  $ hg merge -r large-id
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  normal

Ancestor: large   Parent: large-same   Parent: normal  result: normal

  $ hg up -Cqr large-same
  $ hg merge -r normal
  getting changed largefiles
  0 largefiles updated, 0 removed
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  normal

swap

  $ hg up -Cqr normal
  $ hg merge -r large-same
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  normal

Ancestor: large   Parent: large2   Parent: normal  result: ?
(annoying extra prompt ... but it do not do any serious harm)

  $ hg up -Cqr large2
  $ hg merge -r normal
  remote turned local largefile f into a normal file
  keep (l)argefile or use (n)ormal file? l
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  large2

  $ hg up -Cqr large2
  $ echo n | hg merge -r normal --config ui.interactive=Yes
  remote turned local largefile f into a normal file
  keep (l)argefile or use (n)ormal file? n
  getting changed largefiles
  0 largefiles updated, 0 removed
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  normal

swap

  $ hg up -Cqr normal
  $ hg merge -r large2
  remote turned local normal file f into a largefile
  use (l)argefile or keep (n)ormal file? l
  getting changed largefiles
  1 largefiles updated, 0 removed
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  large2

  $ hg up -Cqr normal
  $ echo n | hg merge -r large2 --config ui.interactive=Yes
  remote turned local normal file f into a largefile
  use (l)argefile or keep (n)ormal file? n
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat f
  normal

  $ cd ..

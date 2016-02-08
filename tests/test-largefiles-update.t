This file focuses mainly on updating largefiles in the working
directory (and ".hg/largefiles/dirstate")

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > merge = internal:fail
  > [extensions]
  > largefiles =
  > [extdiff]
  > # for portability:
  > pdiff = sh "$RUNTESTDIR/pdiff"
  > EOF

  $ hg init repo
  $ cd repo

  $ echo large1 > large1
  $ echo large2 > large2
  $ hg add --large large1 large2
  $ echo normal1 > normal1
  $ hg add normal1
  $ hg commit -m '#0'
  $ echo 'large1 in #1' > large1
  $ echo 'normal1 in #1' > normal1
  $ hg commit -m '#1'
  $ hg pdiff -r '.^' --config extensions.extdiff=
  diff -Nru repo.0d9d9b8dc9a3/.hglf/large1 repo/.hglf/large1
  --- repo.0d9d9b8dc9a3/.hglf/large1	* (glob)
  +++ repo/.hglf/large1	* (glob)
  @@ -1* +1* @@ (glob)
  -4669e532d5b2c093a78eca010077e708a071bb64
  +58e24f733a964da346e2407a2bee99d9001184f5
  diff -Nru repo.0d9d9b8dc9a3/normal1 repo/normal1
  --- repo.0d9d9b8dc9a3/normal1	* (glob)
  +++ repo/normal1	* (glob)
  @@ -1* +1* @@ (glob)
  -normal1
  +normal1 in #1
  [1]
  $ hg update -q -C 0
  $ echo 'large2 in #2' > large2
  $ hg commit -m '#2'
  created new head

Test that update also updates the lfdirstate of 'unsure' largefiles after
hashing them:

The previous operations will usually have left us with largefiles with a mtime
within the same second as the dirstate was written.
The lfdirstate entries will thus have been written with an invalidated/unset
mtime to make sure further changes within the same second is detected.
We will however occasionally be "lucky" and get a tick between writing
largefiles and writing dirstate so we get valid lfdirstate timestamps. The
following verification is thus disabled but can be verified manually.

#if false
  $ hg debugdirstate --large --nodate
  n 644          7 unset               large1
  n 644         13 unset               large2
#endif

Wait to make sure we get a tick so the mtime of the largefiles become valid.

  $ sleep 1

A linear merge will update standins before performing the actual merge. It will
do a lfdirstate status walk and find 'unset'/'unsure' files, hash them, and
update the corresponding standins.
Verify that it actually marks the clean files as clean in lfdirstate so
we don't have to hash them again next time we update.

  $ hg up
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 other heads for branch "default"
  $ hg debugdirstate --large --nodate
  n 644          7 set                 large1
  n 644         13 set                 large2

Test that lfdirstate keeps track of last modification of largefiles and
prevents unnecessary hashing of content - also after linear/noop update

  $ sleep 1
  $ hg st
  $ hg debugdirstate --large --nodate
  n 644          7 set                 large1
  n 644         13 set                 large2
  $ hg up
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 other heads for branch "default"
  $ hg debugdirstate --large --nodate
  n 644          7 set                 large1
  n 644         13 set                 large2

Test that "hg merge" updates largefiles from "other" correctly

(getting largefiles from "other" normally)

  $ hg status -A large1
  C large1
  $ cat large1
  large1
  $ cat .hglf/large1
  4669e532d5b2c093a78eca010077e708a071bb64
  $ hg merge --config debug.dirstate.delaywrite=2
  getting changed largefiles
  1 largefiles updated, 0 removed
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg status -A large1
  M large1
  $ cat large1
  large1 in #1
  $ cat .hglf/large1
  58e24f733a964da346e2407a2bee99d9001184f5
  $ hg diff -c 1 --nodates .hglf/large1 | grep '^[+-][0-9a-z]'
  -4669e532d5b2c093a78eca010077e708a071bb64
  +58e24f733a964da346e2407a2bee99d9001184f5

(getting largefiles from "other" via conflict prompt)

  $ hg update -q -C 2
  $ echo 'large1 in #3' > large1
  $ echo 'normal1 in #3' > normal1
  $ hg commit -m '#3'
  $ cat .hglf/large1
  e5bb990443d6a92aaf7223813720f7566c9dd05b
  $ hg merge --config debug.dirstate.delaywrite=2 --config ui.interactive=True <<EOF
  > o
  > EOF
  largefile large1 has a merge conflict
  ancestor was 4669e532d5b2c093a78eca010077e708a071bb64
  keep (l)ocal e5bb990443d6a92aaf7223813720f7566c9dd05b or
  take (o)ther 58e24f733a964da346e2407a2bee99d9001184f5? o
  merging normal1
  warning: conflicts while merging normal1! (edit, then use 'hg resolve --mark')
  getting changed largefiles
  1 largefiles updated, 0 removed
  0 files updated, 1 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg status -A large1
  M large1
  $ cat large1
  large1 in #1
  $ cat .hglf/large1
  58e24f733a964da346e2407a2bee99d9001184f5

(merge non-existing largefiles from "other" via conflict prompt -
make sure the following commit doesn't abort in a confusing way when trying to
mark the non-existing file as normal in lfdirstate)

  $ mv .hg/largefiles/58e24f733a964da346e2407a2bee99d9001184f5 .
  $ hg update -q -C 3
  $ hg merge --config largefiles.usercache=not --config debug.dirstate.delaywrite=2 --tool :local --config ui.interactive=True <<EOF
  > o
  > EOF
  largefile large1 has a merge conflict
  ancestor was 4669e532d5b2c093a78eca010077e708a071bb64
  keep (l)ocal e5bb990443d6a92aaf7223813720f7566c9dd05b or
  take (o)ther 58e24f733a964da346e2407a2bee99d9001184f5? o
  getting changed largefiles
  large1: largefile 58e24f733a964da346e2407a2bee99d9001184f5 not available from file:/*/$TESTTMP/repo (glob)
  0 largefiles updated, 0 removed
  0 files updated, 2 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m '1-2-3 testing' --config largefiles.usercache=not
  large1: largefile 58e24f733a964da346e2407a2bee99d9001184f5 not available from local store
  $ hg up -C . --config largefiles.usercache=not
  getting changed largefiles
  large1: largefile 58e24f733a964da346e2407a2bee99d9001184f5 not available from file:/*/$TESTTMP/repo (glob)
  0 largefiles updated, 0 removed
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st large1
  ! large1
  $ hg rollback -q
  $ mv 58e24f733a964da346e2407a2bee99d9001184f5 .hg/largefiles/

Test that "hg revert -r REV" updates largefiles from "REV" correctly

  $ hg update -q -C 3
  $ hg status -A large1
  C large1
  $ cat large1
  large1 in #3
  $ cat .hglf/large1
  e5bb990443d6a92aaf7223813720f7566c9dd05b
  $ hg diff -c 1 --nodates .hglf/large1 | grep '^[+-][0-9a-z]'
  -4669e532d5b2c093a78eca010077e708a071bb64
  +58e24f733a964da346e2407a2bee99d9001184f5
  $ hg revert --no-backup -r 1 --config debug.dirstate.delaywrite=2 large1
  $ hg status -A large1
  M large1
  $ cat large1
  large1 in #1
  $ cat .hglf/large1
  58e24f733a964da346e2407a2bee99d9001184f5

Test that "hg rollback" restores status of largefiles correctly

  $ hg update -C -q
  $ hg remove large1
  $ test -f .hglf/large1
  [1]
  $ hg forget large2
  $ test -f .hglf/large2
  [1]
  $ echo largeX > largeX
  $ hg add --large largeX
  $ cat .hglf/largeX
  
  $ hg commit -m 'will be rollback-ed soon'
  $ echo largeY > largeY
  $ hg add --large largeY
#if windows
  $ hg status -A large1
  large1: * (glob)
#else
  $ hg status -A large1
  large1: No such file or directory
#endif
  $ hg status -A large2
  ? large2
  $ hg status -A largeX
  C largeX
  $ hg status -A largeY
  A largeY
  $ hg rollback
  repository tip rolled back to revision 3 (undo commit)
  working directory now based on revision 3
  $ hg status -A large1
  R large1
  $ test -f .hglf/large1
  [1]
  $ hg status -A large2
  R large2
  $ test -f .hglf/large2
  [1]
  $ hg status -A largeX
  A largeX
  $ cat .hglf/largeX
  
  $ hg status -A largeY
  ? largeY
  $ test -f .hglf/largeY
  [1]

Test that "hg rollback" restores standins correctly

  $ hg commit -m 'will be rollback-ed soon'
  $ hg update -q -C 2
  $ cat large1
  large1
  $ cat .hglf/large1
  4669e532d5b2c093a78eca010077e708a071bb64
  $ cat large2
  large2 in #2
  $ cat .hglf/large2
  3cfce6277e7668985707b6887ce56f9f62f6ccd9

  $ hg rollback -q -f
  $ cat large1
  large1
  $ cat .hglf/large1
  4669e532d5b2c093a78eca010077e708a071bb64
  $ cat large2
  large2 in #2
  $ cat .hglf/large2
  3cfce6277e7668985707b6887ce56f9f62f6ccd9

(rollback the parent of the working directory, when the parent of it
is not branch-tip)

  $ hg update -q -C 1
  $ cat .hglf/large1
  58e24f733a964da346e2407a2bee99d9001184f5
  $ cat .hglf/large2
  1deebade43c8c498a3c8daddac0244dc55d1331d

  $ echo normalX > normalX
  $ hg add normalX
  $ hg commit -m 'will be rollback-ed soon'
  $ hg rollback -q

  $ cat .hglf/large1
  58e24f733a964da346e2407a2bee99d9001184f5
  $ cat .hglf/large2
  1deebade43c8c498a3c8daddac0244dc55d1331d

Test that "hg status" shows status of largefiles correctly just after
automated commit like rebase/transplant

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > rebase =
  > strip =
  > transplant =
  > EOF
  $ hg update -q -C 1
  $ hg remove large1
  $ echo largeX > largeX
  $ hg add --large largeX
  $ hg commit -m '#4'

  $ hg rebase -s 1 -d 2 --keep
  rebasing 1:72518492caa6 "#1"
  rebasing 4:07d6153b5c04 "#4" (tip)
#if windows
  $ hg status -A large1
  large1: * (glob)
#else
  $ hg status -A large1
  large1: No such file or directory
#endif
  $ hg status -A largeX
  C largeX
  $ hg strip -q 5

  $ hg update -q -C 2
  $ hg transplant -q 1 4
#if windows
  $ hg status -A large1
  large1: * (glob)
#else
  $ hg status -A large1
  large1: No such file or directory
#endif
  $ hg status -A largeX
  C largeX
  $ hg strip -q 5

  $ hg update -q -C 2
  $ hg transplant -q --merge 1 --merge 4
#if windows
  $ hg status -A large1
  large1: * (glob)
#else
  $ hg status -A large1
  large1: No such file or directory
#endif
  $ hg status -A largeX
  C largeX
  $ hg strip -q 5

Test that linear merge can detect modification (and conflict) correctly

(linear merge without conflict)

  $ echo 'large2 for linear merge (no conflict)' > large2
  $ hg update 3 --config debug.dirstate.delaywrite=2
  getting changed largefiles
  1 largefiles updated, 0 removed
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status -A large2
  M large2
  $ cat large2
  large2 for linear merge (no conflict)
  $ cat .hglf/large2
  9c4bf8f1b33536d6e5f89447e10620cfe52ea710

(linear merge with conflict, choosing "other")

  $ hg update -q -C 2
  $ echo 'large1 for linear merge (conflict)' > large1
  $ hg update 3 --config ui.interactive=True <<EOF
  > o
  > EOF
  largefile large1 has a merge conflict
  ancestor was 4669e532d5b2c093a78eca010077e708a071bb64
  keep (l)ocal ba94c2efe5b7c5e0af8d189295ce00553b0612b7 or
  take (o)ther e5bb990443d6a92aaf7223813720f7566c9dd05b? o
  getting changed largefiles
  1 largefiles updated, 0 removed
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg status -A large1
  C large1
  $ cat large1
  large1 in #3
  $ cat .hglf/large1
  e5bb990443d6a92aaf7223813720f7566c9dd05b

(linear merge with conflict, choosing "local")

  $ hg update -q -C 2
  $ echo 'large1 for linear merge (conflict)' > large1
  $ hg update 3 --config debug.dirstate.delaywrite=2
  largefile large1 has a merge conflict
  ancestor was 4669e532d5b2c093a78eca010077e708a071bb64
  keep (l)ocal ba94c2efe5b7c5e0af8d189295ce00553b0612b7 or
  take (o)ther e5bb990443d6a92aaf7223813720f7566c9dd05b? l
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg status -A large1
  M large1
  $ cat large1
  large1 for linear merge (conflict)
  $ cat .hglf/large1
  ba94c2efe5b7c5e0af8d189295ce00553b0612b7

Test a linear merge to a revision containing same-name normal file

  $ hg update -q -C 3
  $ hg remove large2
  $ echo 'large2 as normal file' > large2
  $ hg add large2
  $ echo 'large3 as normal file' > large3
  $ hg add large3
  $ hg commit -m '#5'
  $ hg manifest
  .hglf/large1
  large2
  large3
  normal1

(modified largefile is already switched to normal)

  $ hg update -q -C 2
  $ echo 'modified large2 for linear merge' > large2
  $ hg update -q 5
  remote turned local largefile large2 into a normal file
  keep (l)argefile or use (n)ormal file? l
  $ hg debugdirstate --nodates | grep large2
  a   0         -1 unset               .hglf/large2
  r   0          0 set                 large2
  $ hg status -A large2
  A large2
  $ cat large2
  modified large2 for linear merge

(added largefile is already committed as normal)

  $ hg update -q -C 2
  $ echo 'large3 as large file for linear merge' > large3
  $ hg add --large large3
  $ hg update -q 5
  remote turned local largefile large3 into a normal file
  keep (l)argefile or use (n)ormal file? l
  $ hg debugdirstate --nodates | grep large3
  a   0         -1 unset               .hglf/large3
  r   0          0 set                 large3
  $ hg status -A large3
  A large3
  $ cat large3
  large3 as large file for linear merge
  $ rm -f large3 .hglf/large3

Test that the internal linear merging works correctly
(both heads are stripped to keep pairing of revision number and commit log)

  $ hg update -q -C 2
  $ hg strip 3 4
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/9530e27857f7-2e7b195d-backup.hg (glob)
  $ mv .hg/strip-backup/9530e27857f7-2e7b195d-backup.hg $TESTTMP

(internal linear merging at "hg pull --update")

  $ echo 'large1 for linear merge (conflict)' > large1
  $ echo 'large2 for linear merge (conflict with normal file)' > large2
  $ hg pull --update --config debug.dirstate.delaywrite=2 $TESTTMP/9530e27857f7-2e7b195d-backup.hg
  pulling from $TESTTMP/9530e27857f7-2e7b195d-backup.hg (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 5 changes to 5 files
  remote turned local largefile large2 into a normal file
  keep (l)argefile or use (n)ormal file? l
  largefile large1 has a merge conflict
  ancestor was 4669e532d5b2c093a78eca010077e708a071bb64
  keep (l)ocal ba94c2efe5b7c5e0af8d189295ce00553b0612b7 or
  take (o)ther e5bb990443d6a92aaf7223813720f7566c9dd05b? l
  2 files updated, 1 files merged, 0 files removed, 0 files unresolved
  1 other heads for branch "default"

  $ hg status -A large1
  M large1
  $ cat large1
  large1 for linear merge (conflict)
  $ cat .hglf/large1
  ba94c2efe5b7c5e0af8d189295ce00553b0612b7
  $ hg status -A large2
  A large2
  $ cat large2
  large2 for linear merge (conflict with normal file)
  $ cat .hglf/large2
  d7591fe9be0f6227d90bddf3e4f52ff41fc1f544

(internal linear merging at "hg unbundle --update")

  $ hg update -q -C 2
  $ hg rollback -q

  $ echo 'large1 for linear merge (conflict)' > large1
  $ echo 'large2 for linear merge (conflict with normal file)' > large2
  $ hg unbundle --update --config debug.dirstate.delaywrite=2 $TESTTMP/9530e27857f7-2e7b195d-backup.hg
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 5 changes to 5 files
  remote turned local largefile large2 into a normal file
  keep (l)argefile or use (n)ormal file? l
  largefile large1 has a merge conflict
  ancestor was 4669e532d5b2c093a78eca010077e708a071bb64
  keep (l)ocal ba94c2efe5b7c5e0af8d189295ce00553b0612b7 or
  take (o)ther e5bb990443d6a92aaf7223813720f7566c9dd05b? l
  2 files updated, 1 files merged, 0 files removed, 0 files unresolved
  1 other heads for branch "default"

  $ hg status -A large1
  M large1
  $ cat large1
  large1 for linear merge (conflict)
  $ cat .hglf/large1
  ba94c2efe5b7c5e0af8d189295ce00553b0612b7
  $ hg status -A large2
  A large2
  $ cat large2
  large2 for linear merge (conflict with normal file)
  $ cat .hglf/large2
  d7591fe9be0f6227d90bddf3e4f52ff41fc1f544

(internal linear merging in subrepo at "hg update")

  $ cd ..
  $ hg init subparent
  $ cd subparent

  $ hg clone -q -u 2 ../repo sub
  $ cat > .hgsub <<EOF
  > sub = sub
  > EOF
  $ hg add .hgsub
  $ hg commit -m '#0@parent'
  $ cat .hgsubstate
  f74e50bd9e5594b7cf1e6c5cbab86ddd25f3ca2f sub
  $ hg -R sub update -q
  $ hg commit -m '#1@parent'
  $ cat .hgsubstate
  d65e59e952a9638e2ce863b41a420ca723dd3e8d sub
  $ hg update -q 0

  $ echo 'large1 for linear merge (conflict)' > sub/large1
  $ echo 'large2 for linear merge (conflict with normal file)' > sub/large2
  $ hg update --config ui.interactive=True --config debug.dirstate.delaywrite=2 <<EOF
  > m
  > r
  > l
  > l
  > EOF
   subrepository sub diverged (local revision: f74e50bd9e55, remote revision: d65e59e952a9)
  (M)erge, keep (l)ocal or keep (r)emote? m
   subrepository sources for sub differ (in checked out version)
  use (l)ocal source (f74e50bd9e55) or (r)emote source (d65e59e952a9)? r
  remote turned local largefile large2 into a normal file
  keep (l)argefile or use (n)ormal file? l
  largefile large1 has a merge conflict
  ancestor was 4669e532d5b2c093a78eca010077e708a071bb64
  keep (l)ocal ba94c2efe5b7c5e0af8d189295ce00553b0612b7 or
  take (o)ther e5bb990443d6a92aaf7223813720f7566c9dd05b? l
  2 files updated, 1 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg -R sub status -A sub/large1
  M sub/large1
  $ cat sub/large1
  large1 for linear merge (conflict)
  $ cat sub/.hglf/large1
  ba94c2efe5b7c5e0af8d189295ce00553b0612b7
  $ hg -R sub status -A sub/large2
  A sub/large2
  $ cat sub/large2
  large2 for linear merge (conflict with normal file)
  $ cat sub/.hglf/large2
  d7591fe9be0f6227d90bddf3e4f52ff41fc1f544

  $ cd ..
  $ cd repo

Test that rebase updates largefiles in the working directory even if
it is aborted by conflict.

  $ hg update -q -C 3
  $ cat .hglf/large1
  e5bb990443d6a92aaf7223813720f7566c9dd05b
  $ cat large1
  large1 in #3
  $ hg rebase -s 1 -d 3 --keep --config ui.interactive=True <<EOF
  > o
  > EOF
  rebasing 1:72518492caa6 "#1"
  largefile large1 has a merge conflict
  ancestor was 4669e532d5b2c093a78eca010077e708a071bb64
  keep (l)ocal e5bb990443d6a92aaf7223813720f7566c9dd05b or
  take (o)ther 58e24f733a964da346e2407a2bee99d9001184f5? o
  merging normal1
  warning: conflicts while merging normal1! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat .hglf/large1
  58e24f733a964da346e2407a2bee99d9001184f5
  $ cat large1
  large1 in #1

Test that rebase updates standins for manually modified largefiles at
the 1st commit of resuming.

  $ echo "manually modified before 'hg rebase --continue'" > large1
  $ hg resolve -m normal1
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue --config ui.interactive=True <<EOF
  > c
  > EOF
  rebasing 1:72518492caa6 "#1"
  rebasing 4:07d6153b5c04 "#4"
  local changed .hglf/large1 which remote deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? c

  $ hg diff -c "tip~1" --nodates .hglf/large1 | grep '^[+-][0-9a-z]'
  -e5bb990443d6a92aaf7223813720f7566c9dd05b
  +8a4f783556e7dea21139ca0466eafce954c75c13
  $ rm -f large1
  $ hg update -q -C tip
  $ cat large1
  manually modified before 'hg rebase --continue'

Test that transplant updates largefiles, of which standins are safely
changed, even if it is aborted by conflict of other.

  $ hg update -q -C 5
  $ cat .hglf/large1
  e5bb990443d6a92aaf7223813720f7566c9dd05b
  $ cat large1
  large1 in #3
  $ hg diff -c 4 .hglf/largeX | grep '^[+-][0-9a-z]'
  +fa44618ea25181aff4f48b70428294790cec9f61
  $ hg transplant 4
  applying 07d6153b5c04
  patching file .hglf/large1
  Hunk #1 FAILED at 0
  1 out of 1 hunks FAILED -- saving rejects to file .hglf/large1.rej
  patch failed to apply
  abort: fix up the working directory and run hg transplant --continue
  [255]
  $ hg status -A large1
  C large1
  $ cat .hglf/large1
  e5bb990443d6a92aaf7223813720f7566c9dd05b
  $ cat large1
  large1 in #3
  $ hg status -A largeX
  A largeX
  $ cat .hglf/largeX
  fa44618ea25181aff4f48b70428294790cec9f61
  $ cat largeX
  largeX

Test that transplant updates standins for manually modified largefiles
at the 1st commit of resuming.

  $ echo "manually modified before 'hg transplant --continue'" > large1
  $ hg transplant --continue
  07d6153b5c04 transplanted as f1bf30eb88cc
  $ hg diff -c tip .hglf/large1 | grep '^[+-][0-9a-z]'
  -e5bb990443d6a92aaf7223813720f7566c9dd05b
  +6a4f36d4075fbe0f30ec1d26ca44e63c05903671
  $ rm -f large1
  $ hg update -q -C tip
  $ cat large1
  manually modified before 'hg transplant --continue'

Test that "hg status" doesn't show removal of largefiles not managed
in the target context.

  $ hg update -q -C 4
  $ hg remove largeX
  $ hg status -A largeX
  R largeX
  $ hg status -A --rev '.^1' largeX

#if execbit

Test that "hg status" against revisions other than parent notices exec
bit changes of largefiles.

  $ hg update -q -C 4

(the case that large2 doesn't have exec bit in the target context but
in the working context)

  $ chmod +x large2
  $ hg status -A --rev 0 large2
  M large2
  $ hg commit -m 'chmod +x large2'

(the case that large2 has exec bit in the target context but not in
the working context)

  $ echo dummy > dummy
  $ hg add dummy
  $ hg commit -m 'revision for separation'
  $ chmod -x large2
  $ hg status -A --rev '.^1' large2
  M large2

#else

Test that "hg status" against revisions other than parent ignores exec
bit correctly on the platform being unaware of it.

  $ hg update -q -C 4

  $ cat > exec-bit.patch <<EOF
  > # HG changeset patch
  > # User test
  > # Date 0 0
  > #      Thu Jan 01 00:00:00 1970 +0000
  > # Node ID be1b433a65b12b27b5519d92213e14f7e1769b90
  > # Parent  07d6153b5c04313efb75deec9ba577de7faeb727
  > chmod +x large2
  > 
  > diff --git a/.hglf/large2 b/.hglf/large2
  > old mode 100644
  > new mode 100755
  > EOF
  $ hg import --exact --bypass exec-bit.patch
  applying exec-bit.patch
  $ hg status -A --rev tip large2
  C large2

#endif

  $ cd ..

Test that "hg convert" avoids copying largefiles from the working
directory into store, because "hg convert" doesn't update largefiles
in the working directory (removing files under ".cache/largefiles"
forces "hg convert" to copy corresponding largefiles)

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > convert =
  > EOF

  $ rm $TESTTMP/.cache/largefiles/6a4f36d4075fbe0f30ec1d26ca44e63c05903671
  $ hg convert -q repo repo.converted

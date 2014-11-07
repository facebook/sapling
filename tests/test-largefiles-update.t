This file focuses mainly on updating largefiles in the working
directory (and ".hg/largefiles/dirstate")

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > merge = internal:fail
  > [extensions]
  > largefiles =
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
  $ hg update -q -C 0
  $ echo 'large2 in #2' > large2
  $ hg commit -m '#2'
  created new head

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
  warning: conflicts during merge.
  merging normal1 incomplete! (edit conflicts, then use 'hg resolve --mark')
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
  local changed .hglf/large2 which remote deleted
  use (c)hanged version or (d)elete? c
  remote turned local largefile large2 into a normal file
  keep (l)argefile or use (n)ormal file? l
  $ hg debugdirstate --nodates | grep large2
  a   0         -1 .hglf/large2
  r   0          0 large2
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
  a   0         -1 .hglf/large3
  r   0          0 large3
  $ hg status -A large3
  A large3
  $ cat large3
  large3 as large file for linear merge
  $ rm -f large3 .hglf/large3

Test that the internal linear merging works correctly
(both heads are stripped to keep pairing of revision number and commit log)

  $ hg update -q -C 2
  $ hg strip 3 4
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/9530e27857f7-backup.hg (glob)
  $ mv .hg/strip-backup/9530e27857f7-backup.hg $TESTTMP

(internal linear merging at "hg pull --update")

  $ echo 'large1 for linear merge (conflict)' > large1
  $ echo 'large2 for linear merge (conflict with normal file)' > large2
  $ hg pull --update --config debug.dirstate.delaywrite=2 $TESTTMP/9530e27857f7-backup.hg
  pulling from $TESTTMP/9530e27857f7-backup.hg (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 5 changes to 5 files
  local changed .hglf/large2 which remote deleted
  use (c)hanged version or (d)elete? c
  remote turned local largefile large2 into a normal file
  keep (l)argefile or use (n)ormal file? l
  largefile large1 has a merge conflict
  ancestor was 4669e532d5b2c093a78eca010077e708a071bb64
  keep (l)ocal ba94c2efe5b7c5e0af8d189295ce00553b0612b7 or
  take (o)ther e5bb990443d6a92aaf7223813720f7566c9dd05b? l
  2 files updated, 1 files merged, 0 files removed, 0 files unresolved

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
  $ hg unbundle --update --config debug.dirstate.delaywrite=2 $TESTTMP/9530e27857f7-backup.hg
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 5 changes to 5 files
  local changed .hglf/large2 which remote deleted
  use (c)hanged version or (d)elete? c
  remote turned local largefile large2 into a normal file
  keep (l)argefile or use (n)ormal file? l
  largefile large1 has a merge conflict
  ancestor was 4669e532d5b2c093a78eca010077e708a071bb64
  keep (l)ocal ba94c2efe5b7c5e0af8d189295ce00553b0612b7 or
  take (o)ther e5bb990443d6a92aaf7223813720f7566c9dd05b? l
  2 files updated, 1 files merged, 0 files removed, 0 files unresolved

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
  > c
  > l
  > l
  > EOF
   subrepository sub diverged (local revision: f74e50bd9e55, remote revision: d65e59e952a9)
  (M)erge, keep (l)ocal or keep (r)emote? m
   subrepository sources for sub differ (in checked out version)
  use (l)ocal source (f74e50bd9e55) or (r)emote source (d65e59e952a9)? r
  local changed .hglf/large2 which remote deleted
  use (c)hanged version or (d)elete? c
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
  largefile large1 has a merge conflict
  ancestor was 4669e532d5b2c093a78eca010077e708a071bb64
  keep (l)ocal e5bb990443d6a92aaf7223813720f7566c9dd05b or
  take (o)ther 58e24f733a964da346e2407a2bee99d9001184f5? o
  merging normal1
  warning: conflicts during merge.
  merging normal1 incomplete! (edit conflicts, then use 'hg resolve --mark')
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
  $ hg rebase --continue --config ui.interactive=True <<EOF
  > c
  > EOF
  local changed .hglf/large1 which remote deleted
  use (c)hanged version or (d)elete? c

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
  abort: fix up the merge and run hg transplant --continue
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

#endif

  $ cd ..

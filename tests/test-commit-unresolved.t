  $ addcommit () {
  >     echo $1 > $1
  >     hg add $1
  >     hg commit -d "${2} 0" -m $1
  > }

  $ commit () {
  >     hg commit -d "${2} 0" -m $1
  > }

  $ hg init a
  $ cd a
  $ addcommit "A" 0
  $ addcommit "B" 1
  $ echo "C" >> A
  $ commit "C" 2

  $ hg update -C 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "D" >> A
  $ commit "D" 3
  created new head

Merging a conflict araises

  $ hg merge
  merging A
  warning: conflicts while merging A! (edit, then use 'hg resolve --mark')
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

Correct the conflict without marking the file as resolved

  $ echo "ABCD" > A
  $ hg commit -m "Merged"
  abort: unresolved merge conflicts (see "hg help resolve")
  [255]

Mark the conflict as resolved and commit

  $ hg resolve -m A
  (no more unresolved files)
  $ hg commit -m "Merged"

Test that if a file is removed but not marked resolved, the commit still fails
(issue4972)

  $ hg up ".^"
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge 2
  merging A
  warning: conflicts while merging A! (edit, then use 'hg resolve --mark')
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg rm --force A
  $ hg commit -m merged
  abort: unresolved merge conflicts (see "hg help resolve")
  [255]

  $ hg resolve -ma
  (no more unresolved files)
  $ hg commit -m merged
  created new head

  $ cd ..

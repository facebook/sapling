run only on case-insensitive filesystems

  $ "$TESTDIR/hghave" icasefs || exit 80

################################
test for branch merging
################################

test for rename awareness of case-folding collision check:

(1) colliding file is one renamed from collided file:
this is also case for issue3370.

  $ hg init merge_renameaware_1
  $ cd merge_renameaware_1

  $ echo a > a
  $ hg add a
  $ hg commit -m '#0'
  $ hg rename a tmp
  $ hg rename tmp A
  $ hg commit -m '#1'
  $ hg update 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'modified at #2' > a
  $ hg commit -m '#2'
  created new head

  $ hg merge
  merging a and A to A
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg status -A
  M A
    a
  R a
  $ cat A
  modified at #2

  $ hg update --clean 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge
  merging A and a to A
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg status -A
  M A
    a
  $ cat A
  modified at #2

  $ cd ..

(2) colliding file is not related to collided file

  $ hg init merge_renameaware_2
  $ cd merge_renameaware_2

  $ echo a > a
  $ hg add a
  $ hg commit -m '#0'
  $ hg remove a
  $ hg commit -m '#1'
  $ echo A > A
  $ hg add A
  $ hg commit -m '#2'
  $ hg update --clean 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'modified at #3' > a
  $ hg commit -m '#3'
  created new head

  $ hg merge
  abort: case-folding collision between A and a
  [255]
  $ hg parents --template '{rev}\n'
  3
  $ hg status -A
  C a
  $ cat a
  modified at #3

  $ hg update --clean 2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge
  abort: case-folding collision between a and A
  [255]
  $ hg parents --template '{rev}\n'
  2
  $ hg status -A
  C A
  $ cat A
  A

  $ cd ..


################################
test for linear updates
################################

test for rename awareness of case-folding collision check:

(1) colliding file is one renamed from collided file

  $ hg init linearupdate_renameaware_1
  $ cd linearupdate_renameaware_1

  $ echo a > a
  $ hg add a
  $ hg commit -m '#0'
  $ hg rename a tmp
  $ hg rename tmp A
  $ hg commit -m '#1'

  $ hg update 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo 'this is added line' >> a
  $ hg update 1
  merging a and A to A
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg status -A
  M A
  $ cat A
  a
  this is added line

  $ cd ..

(2) colliding file is not related to collided file

  $ hg init linearupdate_renameaware_2
  $ cd linearupdate_renameaware_2

  $ echo a > a
  $ hg add a
  $ hg commit -m '#0'
  $ hg remove a
  $ hg commit -m '#1'
  $ echo A > A
  $ hg add A
  $ hg commit -m '#2'

  $ hg update 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg parents --template '{rev}\n'
  0
  $ hg status -A
  C a
  $ cat A
  a
  $ hg up -qC 2

  $ hg update --check 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg parents --template '{rev}\n'
  0
  $ hg status -A
  C a
  $ cat a
  a

  $ hg update --clean 2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg parents --template '{rev}\n'
  2
  $ hg status -A
  C A
  $ cat A
  A

  $ cd ..

(3) colliding file is not related to collided file: added in working dir

  $ hg init linearupdate_renameaware_3
  $ cd linearupdate_renameaware_3

  $ echo a > a
  $ hg add a
  $ hg commit -m '#0'
  $ hg rename a b
  $ hg commit -m '#1'
  $ hg update 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo B > B
  $ hg add B
  $ hg status
  A B
  $ hg update
  abort: case-folding collision between b and B
  [255]

  $ hg update --check
  abort: uncommitted local changes
  [255]

  $ hg update --clean
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg parents --template '{rev}\n'
  1
  $ hg status -A
  C b
  $ cat b
  a

  $ cd ..

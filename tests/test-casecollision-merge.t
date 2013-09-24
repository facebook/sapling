run only on case-insensitive filesystems

  $ "$TESTDIR/hghave" icasefs || exit 80

################################
test for branch merging
################################

test for rename awareness of case-folding collision check:

(1) colliding file is one renamed from collided file:
this is also case for issue3370.

  $ hg init branch_merge_renaming
  $ cd branch_merge_renaming

  $ echo a > a
  $ hg add a
  $ echo b > b
  $ hg add b
  $ hg commit -m '#0'
  $ hg tag -l A
  $ hg rename a tmp
  $ hg rename tmp A
  $ hg commit -m '#1'
  $ hg tag -l B
  $ hg update -q 0
  $ touch x
  $ hg add x
  $ hg commit -m '#2'
  created new head
  $ hg tag -l C

  $ hg merge -q
  $ hg status -A
  M A
  R a
  C b
  C x

  $ hg update -q --clean 1
  $ hg merge -q
  $ hg status -A
  M x
  C A
  C b
  $ hg commit -m '(D)'
  $ hg tag -l D

additional test for issue3452:

| this assumes the history below.
|
|  (A) -- (C) -- (E) -------
|      \      \             \
|       \      \             \
|         (B) -- (D) -- (F) -- (G)
|
|   A: add file 'a'
|   B: rename from 'a' to 'A'
|   C: add 'x' (or operation other than modification of 'a')
|   D: merge C into B
|   E: modify 'a'
|   F: modify 'A'
|   G: merge E into F
|
| issue3452 occurs when (B) is recorded before (C)

  $ hg update -q --clean C
  $ echo "modify 'a' at (E)" > a
  $ echo "modify 'b' at (E)" > b
  $ hg commit -m '(E)'
  created new head
  $ hg tag -l E

  $ hg update -q --clean D
  $ echo "modify 'A' at (F)" > A
  $ hg commit -m '(F)'
  $ hg tag -l F

  $ hg merge -q --tool internal:other E
  $ hg status -A
  M A
    a
  M b
  C x
  $ cat A
  modify 'a' at (E)

test also the case that (B) is recorded after (C), to prevent
regression by changes in the future.

to avoid unexpected (successful) behavior by filelog unification,
target file is not 'a'/'A' but 'b'/'B' in this case.

  $ hg update -q --clean A
  $ hg rename b tmp
  $ hg rename tmp B
  $ hg commit -m '(B1)'
  created new head
  $ hg tag -l B1

  $ hg merge -q C
  $ hg status -A
  M x
  C B
  C a
  $ hg commit -m '(D1)'
  $ hg tag -l D1

  $ echo "modify 'B' at (F1)" > B
  $ hg commit -m '(F1)'
  $ hg tag -l F1

  $ hg merge -q --tool internal:other E
  $ hg status -A
  M B
    b
  M a
  C x
  $ cat B
  modify 'b' at (E)

  $ cd ..

(2) colliding file is not related to collided file

  $ hg init branch_merge_collding
  $ cd branch_merge_collding

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
  $ echo x > x
  $ hg add x
  $ hg commit -m '#3'
  created new head
  $ echo 'modified at #4' > a
  $ hg commit -m '#4'

  $ hg merge
  abort: case-folding collision between a and A
  [255]
  $ hg parents --template '{rev}\n'
  4
  $ hg status -A
  C a
  C x
  $ cat a
  modified at #4

  $ hg update --clean 2
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg merge
  abort: case-folding collision between a and A
  [255]
  $ hg parents --template '{rev}\n'
  2
  $ hg status -A
  C A
  $ cat A
  A

test for deletion awareness of case-folding collision check (issue3648):
revision '#3' doesn't change 'a', so 'a' should be recognized as
safely removed in merging between #2 and #3.

  $ hg update --clean 3
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge 2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg status -A
  M A
  R a
  C x

  $ hg update --clean 2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg status -A
  M x
  C A

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
  abort: uncommitted changes
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

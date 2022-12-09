#require icasefs

################################
test for branch merging
################################

test for rename awareness of case-folding collision check:

(1) colliding file is one renamed from collided file:
this is also case for issue3370.

  $ configure modernclient
  $ newclientrepo branch_merge_renaming

  $ echo a > a
  $ hg add a
  $ echo b > b
  $ hg add b
  $ hg commit -m '#0'
  $ hg rename a tmp
  $ hg rename tmp A
  $ hg commit -m '#1'
  $ hg goto -q 'desc("#0")'
  $ touch x
  $ hg add x
  $ hg commit -m '#2'

  $ hg merge -q
  $ hg status -A
  M A
  R a
  C b
  C x

  $ hg goto -q --clean 'desc("#1")'
  $ hg merge -q
  $ hg status -A
  M x
  C A
  C b
  $ hg commit -m '(D)'

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

  $ hg goto -q --clean 'desc("#2")'
  $ echo "modify 'a' at (E)" > a
  $ echo "modify 'b' at (E)" > b
  $ hg commit -m '(E)'

  $ hg goto -q --clean 'desc("(D)")'
  $ echo "modify 'A' at (F)" > A
  $ hg commit -m '(F)'

  $ hg merge -q --tool internal:other 'desc("(E)")'
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

  $ hg goto -q --clean 'desc("#0")'
  $ hg rename b tmp
  $ hg rename tmp B
  $ hg commit -m '(B1)'

  $ hg merge -q 'desc("#2")'
  $ hg status -A
  M x
  C B
  C a
  $ hg commit -m '(D1)'

  $ echo "modify 'B' at (F1)" > B
  $ hg commit -m '(F1)'

  $ hg merge -q --tool internal:other 'desc("(E)")'
  $ hg status -A
  M B
    b
  M a
  C x
  $ cat B
  modify 'b' at (E)

  $ cd ..

(2) colliding file is not related to collided file

  $ newclientrepo branch_merge_collding

  $ echo a > a
  $ hg add a
  $ hg commit -m '#0'
  $ hg remove a
  $ hg commit -m '#1'
  $ echo A > A
  $ hg add A
  $ hg commit -m '#2'
  $ hg goto --clean 'desc("#0")'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo x > x
  $ hg add x
  $ hg commit -m '#3'
  $ echo 'modified at #4' > a
  $ hg commit -m '#4'

  $ hg merge
  abort: case-folding collision between [aA] and [Aa] (re)
  [255]
  $ hg parents --template '{node}\n'
  5cde3e8ada523402ae83d66767be9332f0fc1c80
  $ hg status -A
  C a
  C x
  $ cat a
  modified at #4

  $ hg goto --clean 'desc("#2")'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg merge
  abort: case-folding collision between [aA] and [Aa] (re)
  [255]
  $ hg parents --template '{node}\n'
  f2bf84af4a6f05abc8d9f0e760d5081bb737b00a
  $ hg status -A
  C A
  $ cat A
  A

test for deletion awareness of case-folding collision check (issue3648):
revision '#3' doesn't change 'a', so 'a' should be recognized as
safely removed in merging between #2 and #3.

  $ hg goto --clean 'desc("#3")'
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge 'desc("#2")'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg status -A
  M A
  R a
  C x

  $ hg goto --clean 'desc("#2")'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge 'desc("#3")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg status -A
  M x
  C A

  $ cd ..

Prepare for tests of directory case-folding collisions

  $ newclientrepo directory-casing
  $ touch 0 # test: file without directory
  $ mkdir 0a
  $ touch 0a/f
  $ mkdir aA
  $ touch aA/a
  $ hg ci -Aqm0

Directory/file case-folding collision:

  $ hg up -q null
  $ touch 00 # test: starts as '0'
  $ mkdir 000 # test: starts as '0'
  $ touch 000/f
  $ touch Aa # test: collision with 'aA/a'
  $ hg ci -Aqm1

  $ hg merge 'desc("0")'
  abort: case-folding collision between Aa and directory of aA/a
  [255]
(note: no collision between 0 and 00 or 000/f)

Directory case-folding collision:

  $ hg up -qC null
  $ hg purge
  $ mkdir 0A0
  $ touch 0A0/f # test: starts as '0a'
  $ mkdir Aa
  $ touch Aa/b # test: collision with 'aA/a'
  $ hg ci -Aqm2

  $ hg merge 'desc("0")'
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ cd ..

################################
test for linear updates
################################

test for rename awareness of case-folding collision check:

(1) colliding file is one renamed from collided file

  $ newclientrepo linearupdate_renameaware_1

  $ echo a > a
  $ hg add a
  $ hg commit -m '#0'
  $ hg rename a tmp
  $ hg rename tmp A
  $ hg commit -m '#1'

  $ hg goto 'desc("#0")'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo 'this is added line' >> a
  $ hg goto 'desc("#1")'
  merging a and A to A
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg status -A
  M A
  $ cat A
  a
  this is added line

  $ cd ..

(2) colliding file is not related to collided file

  $ newclientrepo linearupdate_renameaware_2

  $ echo a > a
  $ hg add a
  $ hg commit -m '#0'
  $ hg remove a
  $ hg commit -m '#1'
  $ echo A > A
  $ hg add A
  $ hg commit -m '#2'

  $ hg goto 'desc("#0")'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg parents --template '{node}\n'
  bf5e395ced2c2bd0d77b2b1d06907af3a19a7836
  $ hg status -A
  C a
  $ cat A
  a
  $ hg up -qC 'desc("#2")'

  $ hg goto --check 'desc("#0")'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg parents --template '{node}\n'
  bf5e395ced2c2bd0d77b2b1d06907af3a19a7836
  $ hg status -A
  C a
  $ cat a
  a

  $ hg goto --clean 'desc("#2")'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg parents --template '{node}\n'
  f2bf84af4a6f05abc8d9f0e760d5081bb737b00a
  $ hg status -A
  C A
  $ cat A
  A

  $ cd ..

(3) colliding file is not related to collided file: added in working dir

  $ newclientrepo linearupdate_renameaware_3

  $ echo a > a
  $ hg add a
  $ hg commit -m '#0'
  $ hg rename a b
  $ hg commit -m '#1'
  $ hg goto 'desc("#0")'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo B > B
  $ hg add B
  warning: possible case-folding collision for B (?)
  $ hg status
  A B
  $ hg goto
  abort: case-folding collision between [bB] and [Bb] (re)
  [255]

  $ hg goto --check
  abort: uncommitted changes
  [255]

  $ hg goto --clean
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg parents --template '{node}\n'
  e419dd32c623b89f5fd3b5870b5df6a3666d4d3a
  $ hg status -A
  C b
  $ cat b
  a

  $ cd ..

  $ hg init basic
  $ cd basic

should complain

  $ hg backout
  abort: please specify a revision to backout
  [255]
  $ hg backout -r 0 0
  abort: please specify just one revision
  [255]

basic operation

  $ echo a > a
  $ hg commit -d '0 0' -A -m a
  adding a
  $ echo b >> a
  $ hg commit -d '1 0' -m b

  $ hg backout -d '2 0' tip --tool=true
  reverting a
  changeset 2:2929462c3dff backs out changeset 1:a820f4f40a57
  $ cat a
  a

file that was removed is recreated

  $ cd ..
  $ hg init remove
  $ cd remove

  $ echo content > a
  $ hg commit -d '0 0' -A -m a
  adding a

  $ hg rm a
  $ hg commit -d '1 0' -m b

  $ hg backout -d '2 0' tip --tool=true
  adding a
  changeset 2:de31bdc76c0d backs out changeset 1:76862dcce372
  $ cat a
  content

backout of backout is as if nothing happened

  $ hg backout -d '3 0' --merge tip --tool=true
  removing a
  changeset 3:7f6d0f120113 backs out changeset 2:de31bdc76c0d
  $ test -f a
  [1]

across branch

  $ cd ..
  $ hg init branch
  $ cd branch
  $ echo a > a
  $ hg ci -Am0
  adding a
  $ echo b > b
  $ hg ci -Am1
  adding b
  $ hg co -C 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

should fail

  $ hg backout 1
  abort: cannot backout change on a different branch
  [255]
  $ echo c > c
  $ hg ci -Am2
  adding c
  created new head

should fail

  $ hg backout 1
  abort: cannot backout change on a different branch
  [255]

backout with merge

  $ cd ..
  $ hg init merge
  $ cd merge

  $ echo line 1 > a
  $ echo line 2 >> a
  $ hg commit -d '0 0' -A -m a
  adding a

remove line 1

  $ echo line 2 > a
  $ hg commit -d '1 0' -m b

  $ echo line 3 >> a
  $ hg commit -d '2 0' -m c

  $ hg backout --merge -d '3 0' 1 --tool=true
  reverting a
  created new head
  changeset 3:26b8ccb9ad91 backs out changeset 1:5a50a024c182
  merging with changeset 3:26b8ccb9ad91
  merging a
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -d '4 0' -m d

check line 1 is back

  $ cat a
  line 1
  line 2
  line 3

  $ cd ..

backout should not back out subsequent changesets

  $ hg init onecs
  $ cd onecs
  $ echo 1 > a
  $ hg commit -d '0 0' -A -m a
  adding a
  $ echo 2 >> a
  $ hg commit -d '1 0' -m b
  $ echo 1 > b
  $ hg commit -d '2 0' -A -m c
  adding b

without --merge
  $ hg backout -d '3 0' 1 --tool=true
  reverting a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg locate b
  b
  $ hg update -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg locate b
  b

with --merge
  $ hg backout --merge -d '3 0' 1 --tool=true
  reverting a
  created new head
  changeset 3:3202beb76721 backs out changeset 1:22bca4c721e5
  merging with changeset 3:3202beb76721
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg locate b
  b
  $ hg update -C tip
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg locate b
  [1]

  $ cd ..
  $ hg init m
  $ cd m
  $ echo a > a
  $ hg commit -d '0 0' -A -m a
  adding a
  $ echo b > b
  $ hg commit -d '1 0' -A -m b
  adding b
  $ echo c > c
  $ hg commit -d '2 0' -A -m b
  adding c
  $ hg update 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo d > d
  $ hg commit -d '3 0' -A -m c
  adding d
  created new head
  $ hg merge 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -d '4 0' -A -m d

backout of merge should fail

  $ hg backout 4
  abort: cannot backout a merge changeset
  [255]

backout of merge with bad parent should fail

  $ hg backout --parent 0 4
  abort: cb9a9f314b8b is not a parent of b2f3bb92043e
  [255]

backout of non-merge with parent should fail

  $ hg backout --parent 0 3
  abort: cannot use --parent on non-merge changeset
  [255]

backout with valid parent should be ok

  $ hg backout -d '5 0' --parent 2 4 --tool=true
  removing d
  changeset 5:10e5328c8435 backs out changeset 4:b2f3bb92043e

  $ hg rollback
  repository tip rolled back to revision 4 (undo commit)
  working directory now based on revision 4
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg backout -d '6 0' --parent 3 4 --tool=true
  removing c
  changeset 5:033590168430 backs out changeset 4:b2f3bb92043e

  $ cd ..

named branches

  $ hg init named_branches
  $ cd named_branches

  $ echo default > default
  $ hg ci -d '0 0' -Am default
  adding default
  $ hg branch branch1
  marked working directory as branch branch1
  (branches are permanent and global, did you want a bookmark?)
  $ echo branch1 > file1
  $ hg ci -d '1 0' -Am file1
  adding file1
  $ hg branch branch2
  marked working directory as branch branch2
  (branches are permanent and global, did you want a bookmark?)
  $ echo branch2 > file2
  $ hg ci -d '2 0' -Am file2
  adding file2

without --merge
  $ hg backout -r 1 --tool=true
  removing file1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch
  branch2
  $ hg status -A
  R file1
  C default
  C file2

with --merge
  $ hg update -qC
  $ hg backout --merge -d '3 0' -r 1 -m 'backout on branch1' --tool=true
  removing file1
  created new head
  changeset 3:d4e8f6db59fb backs out changeset 1:bf1602f437f3
  merging with changeset 3:d4e8f6db59fb
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg update -q -C 2

on branch2 with branch1 not merged, so file1 should still exist:

  $ hg id
  45bbcd363bf0 (branch2)
  $ hg st -A
  C default
  C file1
  C file2

on branch2 with branch1 merged, so file1 should be gone:

  $ hg merge
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -d '4 0' -m 'merge backout of branch1'
  $ hg id
  22149cdde76d (branch2) tip
  $ hg st -A
  C default
  C file2

on branch1, so no file1 and file2:

  $ hg co -C branch1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg id
  bf1602f437f3 (branch1)
  $ hg st -A
  C default
  C file1

  $ cd ..

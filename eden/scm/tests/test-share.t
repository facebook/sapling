
#require killdaemons no-eden

  $ enable share

  $ configure modernclient

prepare repo1

  $ newclientrepo
  $ echo a > a
  $ sl commit -A -m'init'
  adding a

share it

  $ cd ..
  $ sl share repo1 repo2
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

share shouldn't have a store dir

  $ cd repo2
  $ test -d .sl/store
  [1]

Some sed versions appends newline, some don't, and some just fails

  $ cat .sl/sharedpath; echo
  $TESTTMP/repo1/.sl

trailing newline on .sl/sharedpath is ok
  $ sl tip -q
  d3873e73d99e
  $ echo '' >> .sl/sharedpath
  $ cat .sl/sharedpath
  $TESTTMP/repo1/.sl
  $ sl tip -q
  d3873e73d99e

commit in shared clone

  $ echo a >> a
  $ sl commit -m'change in shared clone'

check original

  $ cd ../repo1
  $ sl log
  commit:      8af4dc49db9e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change in shared clone
  
  commit:      d3873e73d99e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     init
  
  $ sl goto tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a             # should be two lines of "a"
  a
  a

commit in original

  $ echo b > b
  $ sl commit -A -m'another file'
  adding b

check in shared clone

  $ cd ../repo2
  $ sl log
  commit:      c2e0ac586386
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     another file
  
  commit:      8af4dc49db9e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change in shared clone
  
  commit:      d3873e73d99e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     init
  
  $ sl goto tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat b             # should exist with one "b"
  b

test unshare command

  $ sl unshare
  $ test -d .sl/store
  $ test -f .sl/sharedpath
  [1]
  $ grep shared .sl/requires
  [1]
  $ sl unshare
  abort: this is not a shared repo
  [255]

check that a change does not propagate

  $ echo b >> b
  $ sl commit -m'change in unshared'
  $ cd ../repo1
  $ sl id -r tip
  c2e0ac586386

  $ cd ..


test sharing bookmarks

  $ sl share -B repo1 repo3
  updating working directory
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo1
  $ sl bookmark bm1
  $ sl bookmarks
   * bm1                       c2e0ac586386
  $ cd ../repo2
  $ sl book bm2
  $ sl bookmarks
   * bm2                       0e6e70d1d5f1
  $ cd ../repo3
  $ sl bookmarks
     bm1                       c2e0ac586386
  $ sl book bm3
  $ sl bookmarks
     bm1                       c2e0ac586386
   * bm3                       c2e0ac586386
  $ cd ../repo1
  $ sl bookmarks
   * bm1                       c2e0ac586386
     bm3                       c2e0ac586386

check whether HG_PENDING makes pending changes only in related
repositories visible to an external hook.

In "sl share" case, another transaction can't run in other
repositories sharing same source repository, because starting
transaction requires locking store of source repository.

Therefore, this test scenario ignores checking visibility of
.sl/bookmarks.pending in repo2, which shares repo1 without bookmarks.

  $ cat > $TESTTMP/checkbookmarks.sh <<EOF
  > echo "@repo1"
  > hg -R "$TESTTMP/repo1" bookmarks
  > echo "@repo2"
  > hg -R "$TESTTMP/repo2" bookmarks
  > echo "@repo3"
  > hg -R "$TESTTMP/repo3" bookmarks
  > exit 1 # to avoid adding new bookmark for subsequent tests
  > EOF

  $ cd ../repo1
  $ sl --config hooks.pretxnclose="sh $TESTTMP/checkbookmarks.sh" -q book bmX
  @repo1
     bm1                       c2e0ac586386
     bm3                       c2e0ac586386
   * bmX                       c2e0ac586386
  @repo2
   * bm2                       0e6e70d1d5f1
  @repo3
     bm1                       c2e0ac586386
   * bm3                       c2e0ac586386
  transaction abort!
  rollback completed
  abort: pretxnclose hook exited with status 1
  [255]
XXX: bmX should show up for repo3.
  $ sl book bm1

In the unshared case, a bookmark being added in repo2 is not visible in repo1.

  $ cd ../repo2
  $ sl --config hooks.pretxnclose="sh $TESTTMP/checkbookmarks.sh" -q book bmX
  @repo1
   * bm1                       c2e0ac586386
     bm3                       c2e0ac586386
  @repo2
     bm2                       0e6e70d1d5f1
   * bmX                       0e6e70d1d5f1
  @repo3
     bm1                       c2e0ac586386
   * bm3                       c2e0ac586386
  transaction abort!
  rollback completed
  abort: pretxnclose hook exited with status 1
  [255]
  $ sl book bm2

In symmetry with the first case, bmX is visible in repo1 (= shared rc)
because HG_SHAREDPENDING refers to repo1.

  $ cd ../repo3
  $ sl --config hooks.pretxnclose="sh $TESTTMP/checkbookmarks.sh" -q book bmX
  @repo1
   * bm1                       c2e0ac586386
     bm3                       c2e0ac586386
     bmX                       c2e0ac586386
  @repo2
   * bm2                       0e6e70d1d5f1
  @repo3
     bm1                       c2e0ac586386
     bm3                       c2e0ac586386
   * bmX                       c2e0ac586386
  transaction abort!
  rollback completed
  abort: pretxnclose hook exited with status 1
  [255]
  $ sl book bm3

  $ cd ../repo1

test that commits work

  $ echo 'shared bookmarks' > a
  $ sl commit -m 'testing shared bookmarks'
  $ sl bookmarks
   * bm1                       b87954705719
     bm3                       c2e0ac586386
  $ cd ../repo3
  $ sl bookmarks
     bm1                       b87954705719
   * bm3                       c2e0ac586386
  $ echo 'more shared bookmarks' > a
  $ sl commit -m 'testing shared bookmarks'
  $ sl bookmarks
     bm1                       b87954705719
   * bm3                       62f4ded848e4
  $ cd ../repo1
  $ sl bookmarks
   * bm1                       b87954705719
     bm3                       62f4ded848e4
  $ cd ..

test behavior when sharing a shared repo

  $ sl share -B repo3 repo5
  updating working directory
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo5
  $ sl book
     bm1                       b87954705719
     bm3                       62f4ded848e4
  $ cd ..

test what happens when an active bookmark is deleted

  $ cd repo1
  $ sl bookmarks -d bm3
  $ sl bookmarks
   * bm1                       b87954705719
  $ cd ../repo3
  $ sl bookmarks
     bm1                       b87954705719
  $ cd ..

verify bookmark behavior after unshare

  $ cd repo3
  $ sl unshare
  $ sl bookmarks
     bm1                       b87954705719
  $ sl bookmarks bm5
  $ sl bookmarks
     bm1                       b87954705719
   * bm5                       62f4ded848e4
  $ cd ../repo1
  $ sl bookmarks
   * bm1                       b87954705719
  $ cd ..

test shared clones using relative paths work

  $ mkdir thisdir
  $ newclientrepo thisdir/orig
  $ cd
  $ sl share -U thisdir/orig thisdir/abs
  $ sl share -U --relative thisdir/abs thisdir/rel
  $ cat thisdir/rel/.sl/sharedpath
  ../../orig/.sl (no-eol)
  $ grep shared thisdir/*/.sl/requires
  thisdir/abs/.sl/requires:shared
  thisdir/rel/.sl/requires:relshared
  thisdir/rel/.sl/requires:shared

test that relative shared paths aren't relative to $PWD

  $ cd thisdir
  $ sl -R rel root
  $TESTTMP/thisdir/rel
  $ cd ..

now test that relative paths really are relative, survive across
renames and changes of PWD

  $ sl -R thisdir/abs root
  $TESTTMP/thisdir/abs
  $ sl -R thisdir/rel root
  $TESTTMP/thisdir/rel
  $ mv thisdir thatdir
  $ sl -R thatdir/abs root
  abort: sharedpath points to nonexistent directory $TESTTMP/thisdir/orig!
  [255]
  $ sl -R thatdir/rel root
  $TESTTMP/thatdir/rel

test unshare relshared repo

  $ cd thatdir/rel
  $ sl unshare
  $ test -d .sl/store
  $ test -f .sl/sharedpath
  [1]
  $ grep shared .sl/requires
  [1]
  $ sl unshare
  abort: this is not a shared repo
  [255]
  $ cd ../..

  $ rm -r thatdir

Explicitly kill daemons to let the test exit on Windows

  $ $PYTHON $TESTDIR/killdaemons.py

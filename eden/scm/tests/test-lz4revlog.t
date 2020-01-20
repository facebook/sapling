  $ disable treemanifest
  $ hg init a
  $ cd a
  $ echo a>a
  $ hg ci -q -A -m 0

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "lz4revlog=" >> $HGRCPATH

having lz4revlog enabled should not affect an existing repo

  $ for i in 0 1 2 3 4 5 6 7 8 9; do
  >   echo qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqquuuuuuuuuuuuuuuuuuuuqqqq$i >> a
  > done
  $ hg ci -q -m 1
  $ hg verify -q

  $ cd ..

regular clone of an existing zlib repo should still use zlib

  $ hg clone a b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sort b/.hg/requires
  dotencode
  fncache
  generaldelta
  revlogv1
  store
  treestate

pulled clone of zlib should use lz4

  $ hg clone -q --pull a alz4
  $ sort alz4/.hg/requires
  dotencode
  fncache
  generaldelta
  lz4revlog
  revlogv1
  store
  treestate

disable lz4, then clone

  $ hg --config format.uselz4=False clone --pull a w
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sort w/.hg/requires
  dotencode
  fncache
  generaldelta
  revlogv1
  store
  treestate

attempt to disable lz4 should be ignored for hardlinked clone

  $ hg --config format.uselz4=False clone alz4 azlib
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sort azlib/.hg/requires
  dotencode
  fncache
  generaldelta
  lz4revlog
  revlogv1
  store
  treestate

a new repo should use lz4 by default

  $ hg init lz
  $ cd lz
  $ echo a>a
  $ touch b
  $ hg ci -q -A -m lz0
  $ for i in 0 1 2 3 4 5 6 7 8 9; do
  >   echo qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqquuuuuuuuuuuuuuuuuuuu$i >> a
  > done
  $ hg ci -q -m lz1
  $ hg verify -q
  $ hg tip
  changeset:   1:18e28922b6ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     lz1
  
  $ sort .hg/requires
  dotencode
  fncache
  generaldelta
  lz4revlog
  revlogv1
  store
  treestate

vanilla hg should bail in an lz4 repo

  $ hg --config 'extensions.lz4revlog=!' tip
  abort: repository requires features unknown to this Mercurial: lz4revlog!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]

start a server

XXX: this should be tested with hghave

  $ hg --config server.uncompressed=True serve -p $HGPORT -d --pid-file=../hg1.pid -E ../error.log
  $ cat ../hg1.pid >> $DAEMON_PIDS

uncompressed clone from lz4 to lz4 should be fine

  $ cd ..
  $ hg clone --uncompressed http://localhost:$HGPORT/ happy
  streaming all changes
  5 files to transfer, * of data (glob)
  transferred 785 bytes in * seconds * (glob)
  searching for changes
  no changes found
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

uncompressed clone from lz4 to non-lz4 should fall back to pull

  $ hg --config 'extensions.lz4revlog=!' clone -U --uncompressed http://localhost:$HGPORT/ nonesuch
  warning: stream clone requested but client is missing requirements: lz4revlog
  (see https://www.mercurial-scm.org/wiki/MissingRequirement for more information)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 3 changes to 2 files

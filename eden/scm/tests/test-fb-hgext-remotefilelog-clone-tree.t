#chg-compatible

  $ disable treemanifest

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ echo treemanifest >> .hg/requires
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
# uppercase directory name to test encoding
  $ mkdir -p A/B
  $ echo x > A/B/x
  $ hg commit -qAm x

  $ cd ..

# shallow clone from full

  $ hgcloneshallow ssh://user@dummy/master shallow --noupdate
  streaming all changes
  5 files to transfer, * of data (glob)
  transferred 449 bytes in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  $ cd shallow
  $ cat .hg/requires
  dotencode
  fncache
  generaldelta
  remotefilelog
  revlogv1
  store
  treemanifest
  treestate
  $ find .hg/store/meta | sort
  .hg/store/meta
  .hg/store/meta/_a
  .hg/store/meta/_a/00manifest.i
  .hg/store/meta/_a/_b
  .hg/store/meta/_a/_b/00manifest.i

  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

  $ cat A/B/x
  x

  $ echo foo > A/B/F
  $ hg add A/B/F
  $ hg ci -m 'local content'

  $ cd ..

# shallow clone from shallow

  $ hgcloneshallow ssh://user@dummy/shallow shallow2  --noupdate
  streaming all changes
  9 files to transfer, * of data (glob)
  transferred 3.20 KB in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  $ cd shallow2
  $ cat .hg/requires
  dotencode
  fncache
  generaldelta
  remotefilelog
  revlogv1
  store
  treemanifest
  treestate
  $ [ -d .hg/store/data ]
  [1]

  $ hg update
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat A/B/x
  x

  $ cd ..

# full clone from shallow
# - send stderr to /dev/null because the order of stdout/err causes
#   flakiness here
  $ hg clone --noupdate ssh://user@dummy/shallow full 2>err
  streaming all changes
  [255]
  $ grep '^remote' err
  remote: abort: Cannot clone from a shallow repo to a full repo.

# getbundle full clone

  $ printf '[server]\npreferuncompressed=False\n' >> master/.hg/hgrc
  $ hgcloneshallow ssh://user@dummy/master shallow3
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat shallow3/.hg/requires
  dotencode
  fncache
  generaldelta
  remotefilelog
  revlogv1
  store
  treemanifest
  treestate

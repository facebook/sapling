#chg-compatible

  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x

  $ cd ..

# shallow clone from full

  $ hgcloneshallow ssh://user@dummy/master shallow --noupdate
  streaming all changes
  3 files to transfer, * of data (glob)
  transferred 227 bytes in * seconds (*/sec) (glob)
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
  treestate

  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

  $ cat x
  x

  $ echo foo > f
  $ hg add f
  $ hg ci -m 'local content'

  $ cd ..

# shallow clone from shallow

  $ hgcloneshallow ssh://user@dummy/shallow shallow2  --noupdate
  streaming all changes
  7 files to transfer, 2.76 KB of data
  transferred 2.76 KB in * seconds (*/sec) (glob)
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
  treestate
  $ [ -d .hg/store/data ]
  [1]

  $ hg update
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat x
  x

  $ cd ..

# full clone from shallow

Note: the output to STDERR comes from a different process to the output on
STDOUT and their relative ordering is not deterministic. As a result, the test
was failing sporadically. To avoid this, we capture STDERR to a file and
check its contents separately.

  $ TEMP_STDERR=full-clone-from-shallow.stderr.tmp
  $ hg clone --noupdate ssh://user@dummy/shallow full 2>$TEMP_STDERR
  streaming all changes
  [255]
- sort because the order is non-deterministic because of stderr pipe buffering
  $ cat $TEMP_STDERR | sort
  abort: unexpected response from remote server: empty string
  remote: abort: Cannot clone from a shallow repo to a full repo.
  $ rm $TEMP_STDERR

# getbundle full clone

  $ printf '[server]\npreferuncompressed=False\n' >> master/.hg/hgrc
  $ hgcloneshallow ssh://user@dummy/master shallow3
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets b292c1e3311f
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat shallow3/.hg/requires
  dotencode
  fncache
  generaldelta
  remotefilelog
  revlogv1
  store
  treestate

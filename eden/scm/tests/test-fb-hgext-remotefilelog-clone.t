#chg-compatible
  $ setconfig experimental.allowfilepeer=True

  $ disable treemanifest
  $ setconfig remotenames.selectivepull=1
  $ setconfig remotefilelog.write-hgcache-to-indexedlog=False remotefilelog.write-local-to-indexedlog=False
  $ setconfig scmstore.enableshim=False

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
  fetching changelog
  3 files to transfer, * of data (glob)
  transferred 227 bytes in * seconds (*/sec) (glob)
  fetching selected remote bookmarks
  $ cd shallow
  $ cat .hg/requires
  dotencode
  fncache
  generaldelta
  lz4revlog
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
  fetching changelog
  7 files to transfer, 2.76 KB of data
  transferred 2.76 KB in * seconds (*/sec) (glob)
  fetching selected remote bookmarks
  $ cd shallow2
  $ cat .hg/requires
  dotencode
  fncache
  generaldelta
  lz4revlog
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
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat shallow3/.hg/requires
  dotencode
  fncache
  generaldelta
  lz4revlog
  remotefilelog
  revlogv1
  store
  treestate

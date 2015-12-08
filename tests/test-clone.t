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
  2 files to transfer, 227 bytes of data
  transferred 227 bytes in 0.0 seconds (* KB/sec) (glob)
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

  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

  $ cat x
  x

  $ cd ..

# shallow clone from shallow

  $ hgcloneshallow ssh://user@dummy/shallow shallow2  --noupdate
  streaming all changes
  2 files to transfer, 227 bytes of data
  transferred 227 bytes in 0.0 seconds (* KB/sec) (glob)
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

  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat x
  x

  $ cd ..

# full clone from shallow

  $ hg clone --noupdate ssh://user@dummy/shallow full
  streaming all changes
  remote: abort: Cannot clone from a shallow repo to a full repo.
  abort: unexpected response from remote server: empty string
  [255]

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

  $ ls shallow3/.hg/store/data
  $ cat shallow3/.hg/requires
  dotencode
  fncache
  generaldelta
  remotefilelog
  revlogv1
  store

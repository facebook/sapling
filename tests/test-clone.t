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

  $ hgcloneshallow ssh://localhost/$PWD/master shallow --noupdate
  streaming all changes
  2 files to transfer, 227 bytes of data
  transferred 227 bytes in 0.0 seconds (* KB/sec) (glob)
  $ cd shallow
  $ cat .hg/requires
  dotencode
  fncache
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

  $ hgcloneshallow ssh://localhost/$PWD/shallow shallow2  --noupdate
  streaming all changes
  2 files to transfer, 227 bytes of data
  transferred 227 bytes in 0.0 seconds (* KB/sec) (glob)
  $ cd shallow2
  $ cat .hg/requires
  dotencode
  fncache
  remotefilelog
  revlogv1
  store

  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cat x
  x

  $ cd ..

# full clone from shallow

  $ hg clone --noupdate ssh://localhost/$PWD/shallow full
  abort: unexpected response from remote server: empty string
  remote: abort: Cannot clone from a shallow repo to a full repo.
  [255]

  $ cd ..

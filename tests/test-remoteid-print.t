  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ . "$TESTDIR/library.sh"

# create a repo to behave as the server

  $ hginit server
  $ cd server
  $ echo x > x
  $ hg commit -qAm x

# enable the remoteid extension

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remoteid=$TESTDIR/../hgext3rd/remoteid.py
  > [remotefilelog]
  > server=True
  > EOF

# clone the server repo - should display "hostname: ..."

  $ cd ..
  $ hgcloneshallow ssh://user@dummy/server client
  remote: hostname: * (glob)
  streaming all changes
  2 files to transfer, 227 bytes of data
  transferred 227 bytes in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  updating to branch default
  remote: hostname: * (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

#chg-compatible

  $ setconfig extensions.treemanifest=!
  $ setconfig discovery.fastdiscovery=True
  $ . $TESTDIR/library.sh
  $ . "$TESTDIR/infinitepush/library.sh"

  $ setupcommon

Setup remotefilelog server
  $ hg init server
  $ cd server
  $ setupserver
  $ setconfig remotefilelog.server=true
  $ mkcommit initial
  $ cd ..

Make client shallow clone
  $ hgcloneshallow ssh://user@dummy/server client
  streaming all changes
  * files to transfer, * bytes of data (glob)
  transferred * bytes in * seconds (* KB/sec) (glob)
  searching for changes
  no changes found
  updating to branch default
  * files updated, * files merged, * files removed, * files unresolved (glob)
  * files fetched over * fetches - (* misses, * hit ratio) over * (glob) (?)

  $ cd server
  $ mkcommit first
  $ mkcommit second
  $ mkcommit third

Make sure that fastdiscovery is used for pull
  $ cd ../client
  $ hg pull --debug | grep fastdiscovery
  using fastdiscovery

Make sure that fastdiscovery is used for push
  $ hg up -q tip
  3 files fetched over 1 fetches - (3 misses, * hit ratio) over * (glob) (?)
  $ mkcommit clientcommit
  $ hg push --debug 2>&1 | grep fastdiscovery || echo "no fastdiscovery"
  no fastdiscovery

Make public head on the client - fastdiscovery is NOT used because no common nodes found
  $ mkcommit publichead
  $ hg phase -r . -p
  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  server has changed since last pull - falling back to the default search strategy
  searching for changes
  no changes found

Set knownserverbookmarks - fastdiscovery is used
  $ hg book -r ".^" master_bookmark
  $ hg pull --config discovery.knownserverbookmarks=master_bookmark
  pulling from ssh://user@dummy/server
  searching for changes
  no changes found

  $ cd ../server
  $ mkcommit newcommit
  $ cd ../client
  $ hg pull --config discovery.knownserverbookmarks=master_bookmark
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 85d8b0ac7dad

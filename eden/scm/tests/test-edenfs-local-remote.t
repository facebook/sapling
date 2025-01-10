#require eden

Defeat shared cache. Fetching non-local repo data will always perform a remote fetch.
  $ setconfig remotefilelog.cachepath=

Create server/remote repo.
  $ newserver server
  $ drawdag <<EOS
  > A # A/remote/file = foo\n
  > # bookmark master = A
  > EOS

  $ cd

Pre-clone backing repo so we can add some local commit data.
  $ hg clone --no-eden -qU test:server backing
  $ cd backing
  $ hg go -q $A
Add some "local only" data not available on server.
  $ mkdir local
  $ echo bar > local/file
  $ hg ci -Aqm B

  $ cd

Now clone the eden working copy.
  $ hg clone -q --eden --eden-backing-repo $TESTTMP/backing test:server working
  $ cd working
First trigger aux data fetches.
  $ ls -l remote/file local/file
  * local/file (glob)
  * remote/file (glob)
Check aux counters.
  $ sleep 1 # seems to be some delay to see updated counters
  $ eden debug thrift getCounters --json | grep -e 'sapling.fetch_blob_metadata_(local|success).count"'
    "store.sapling.fetch_blob_metadata_local.count": 1,
    "store.sapling.fetch_blob_metadata_success.count": 2,
Trigger file content fetches.
  $ cat remote/file
  foo
  $ cat local/file
  bar
Check we got 1 local and 1 remote for each of file and blob.
  $ sleep 1
  $ eden debug thrift getCounters --json | grep -e 'sapling.fetch_(tree|blob)_(local|success).count"'
    "store.sapling.fetch_blob_local.count": 1,
    "store.sapling.fetch_blob_success.count": 2,
    "store.sapling.fetch_tree_local.count": 1,
    "store.sapling.fetch_tree_success.count": 2,

Make sure null commit works.
  $ hg go -q null
  $ ls

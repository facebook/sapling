# Initial setup
  $ setconfig extensions.lfs=
  $ setconfig extensions.snapshot=
  $ setconfig extensions.treemanifest=!

# Prepare server and client repos.
  $ hg init server
  $ hg clone -q server client
  $ cd client

# Add a file to the store
  $ echo "foo" > existingfile
  $ hg commit -Aqm "add some file"
  $ hg push
  pushing to $TESTTMP/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

# No need to create snapshot now
  $ hg debugcreatesnapshotmanifest
  Working copy is even with the last commit. No need to create snapshot.

# Make some changes: add an untracked file and remove the tracked file
  $ echo "bar" > untrackedfile
  $ rm existingfile
  $ OID="$(hg debugcreatesnapshotmanifest | cut -f3 -d' ')"
  $ echo "$OID"
  1f341c81a097100373b4bfe017b80d767d2b74bd434dbfa9ced3c1964024c65d

# Check that the blobstore is populated
  $ find .hg/store/lfs/objects | sort
  .hg/store/lfs/objects
  .hg/store/lfs/objects/1f
  .hg/store/lfs/objects/1f/341c81a097100373b4bfe017b80d767d2b74bd434dbfa9ced3c1964024c65d
  .hg/store/lfs/objects/7d
  .hg/store/lfs/objects/7d/865e959b2466918c9863afca942d0fb89d7c9ac0c99bafc3749504ded97730

# Check the contents of the manifest file
  $ cat .hg/store/lfs/objects/1f/341c81a097100373b4bfe017b80d767d2b74bd434dbfa9ced3c1964024c65d
  {"deleted": {"existingfile": null}, "unknown": {"untrackedfile": {"oid": "7d865e959b2466918c9863afca942d0fb89d7c9ac0c99bafc3749504ded97730", "size": "4"}}} (no-eol)

# Check that the untracked file is stored in lfs
  $ cat .hg/store/lfs/objects/7d/865e959b2466918c9863afca942d0fb89d7c9ac0c99bafc3749504ded97730
  bar

# Upload the manifest contents to server
  $ cat >> $HGRCPATH << EOF
  > [lfs]
  > url=file:$TESTTMP/lfsremote/
  > EOF

  $ hg debuguploadsnapshotmanifest 1f341c81a097100373b4bfe017b80d767d2b74bd434dbfa9ced3c1964024c6b5
  abort: manifest oid 1f341c81a097100373b4bfe017b80d767d2b74bd434dbfa9ced3c1964024c6b5 not found in local blobstorage
  [255]

  $ hg debuguploadsnapshotmanifest "$OID"
  upload complete

# Check the remote storage
  $ find $TESTTMP/lfsremote | sort
  $TESTTMP/lfsremote
  $TESTTMP/lfsremote/1f
  $TESTTMP/lfsremote/1f/341c81a097100373b4bfe017b80d767d2b74bd434dbfa9ced3c1964024c65d
  $TESTTMP/lfsremote/7d
  $TESTTMP/lfsremote/7d/865e959b2466918c9863afca942d0fb89d7c9ac0c99bafc3749504ded97730

  $ cat $TESTTMP/lfsremote/1f/341c81a097100373b4bfe017b80d767d2b74bd434dbfa9ced3c1964024c65d
  {"deleted": {"existingfile": null}, "unknown": {"untrackedfile": {"oid": "7d865e959b2466918c9863afca942d0fb89d7c9ac0c99bafc3749504ded97730", "size": "4"}}} (no-eol)

  $ cat $TESTTMP/lfsremote/7d/865e959b2466918c9863afca942d0fb89d7c9ac0c99bafc3749504ded97730
  bar

# Checkout the manifest
  $ cd ../
  $ hg clone -q server client2
  $ cd client2
  $ hg update
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls
  existingfile

  $ hg debugcheckoutsnapshot "$OID"
  snapshot checkout complete

  $ ls
  untrackedfile

  $ cat untrackedfile
  bar

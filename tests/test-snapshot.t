# Initial setup
  $ setconfig extensions.lfs=
  $ setconfig extensions.snapshot=

# Prepare server and client repos.
  $ hg init server
  $ hg clone -q server client
  $ cd client

# Add a file to the store
  $ echo "foo" > existingfile
  $ hg commit -Aqm "add some file"

# No need to create snapshot now
  $ hg debugcreatesnapshotmanifest
  Working copy is even with the last commit. No need to create snapshot.

# Make some changes: add an untracked file and remove the tracked file
  $ echo "bar" > untrackedfile
  $ rm existingfile
  $ hg debugcreatesnapshotmanifest
  manifest oid: 1f341c81a097100373b4bfe017b80d767d2b74bd434dbfa9ced3c1964024c65d

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

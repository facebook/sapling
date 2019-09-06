# Initial setup
  $ setconfig extensions.snapshot=
  $ setconfig extensions.rebase=
  $ setconfig extensions.treemanifest=!

# Prepare server and client repos.
  $ hg init server
  $ hg clone -q server client
  $ cd client

# Add a file to the store
  $ echo "foo" > existingfile
  $ hg add existingfile
  $ hg commit -m "add some file"
  $ hg push
  pushing to $TESTTMP/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

# No need to create snapshot now
  $ hg debugcreatesnapshotmetadata
  Working copy is even with the last commit. No need to create snapshot.

# Make some changes: add an untracked file and remove the tracked file
  $ echo "bar" > untrackedfile
  $ rm existingfile
  $ OID="$(hg debugcreatesnapshotmetadata | cut -f3 -d' ')"
  $ echo "$OID"
  f62f9175588ac550bc215b56b441de94f6b3c859023f971453057342614db332

# Check that the blobstore is populated
  $ find .hg/store/snapshots/objects | sort
  .hg/store/snapshots/objects
  .hg/store/snapshots/objects/7d
  .hg/store/snapshots/objects/7d/865e959b2466918c9863afca942d0fb89d7c9ac0c99bafc3749504ded97730
  .hg/store/snapshots/objects/f6
  .hg/store/snapshots/objects/f6/2f9175588ac550bc215b56b441de94f6b3c859023f971453057342614db332

# Check the contents of the metadata file
  $ cat .hg/store/snapshots/objects/"${OID:0:2}"/"${OID:2}"
  {"files": {"deleted": {"existingfile": null}, "localvfsfiles": {}, "unknown": {"untrackedfile": {"oid": "7d865e959b2466918c9863afca942d0fb89d7c9ac0c99bafc3749504ded97730", "size": "4"}}}, "version": "1"} (no-eol)

# Check that the untracked file is stored in local storage
  $ cat .hg/store/snapshots/objects/7d/865e959b2466918c9863afca942d0fb89d7c9ac0c99bafc3749504ded97730
  bar

# Checkout the metadata
  $ hg update --clean . && rm untrackedfile
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls
  existingfile

  $ hg debugcheckoutsnapshotmetadata --verbose "$OID"
  will delete existingfile
  will add untrackedfile
  snapshot checkout complete

  $ ls
  untrackedfile

  $ cat untrackedfile
  bar

# Check handling of merge state
  $ cd ../client
  $ hg revert existingfile
  $ echo "a" > new
  $ hg add new
  $ hg commit -m "merge #1"
  $ hg checkout 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "b" > new
  $ hg add new
  $ hg commit -m "merge #2"
  $ hg rebase -d 1
  rebasing 2:1f00a8382720 "merge #2" (tip)
  merging new
  warning: 1 conflicts while merging new! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ rm existingfile
  $ hg status --verbose
  M new
  ! existingfile
  ? new.orig
  ? untrackedfile
  # The repository is in an unfinished *rebase* state.
  
  # Unresolved merge conflicts:
  # 
  #     new
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  
  # To continue:                hg rebase --continue
  # To abort:                   hg rebase --abort
  
# So we have an unfinished rebase state
  $ MERGEOID="$(hg debugcreatesnapshotmetadata | cut -f3 -d' ')"

# Check the result
  $ hg rebase --abort
  rebase aborted
  $ hg update --clean . && rm new.orig
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status
  ? untrackedfile
  $ hg debugcheckoutsnapshotmetadata --verbose "$MERGEOID"
  will delete existingfile
  will add new.orig
  skip adding untrackedfile, it exists
  will add merge/c2a6b03f190dfb2b4aa91f8af8d477a9bc3401dc
  will add merge/state
  will add merge/state2
  will add rebasestate
  snapshot checkout complete
  $ hg status --verbose
  ! existingfile
  ? new.orig
  ? untrackedfile
  # The repository is in an unfinished *rebase* state.
  
  # Unresolved merge conflicts:
  # 
  #     new
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  
  # To continue:                hg rebase --continue
  # To abort:                   hg rebase --abort
  

  $ find .hg/merge | sort
  .hg/merge
  .hg/merge/c2a6b03f190dfb2b4aa91f8af8d477a9bc3401dc
  .hg/merge/state
  .hg/merge/state2

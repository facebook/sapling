# Initial setup
  $ setconfig extensions.rebase=
  $ setconfig extensions.snapshot=
  $ setconfig extensions.treemanifest=!
  $ setconfig visibility.enabled=true

# Prepare server and client repos.
  $ hg init server
  $ hg clone -q server client
  $ cd client
  $ hg debugvisibility start

# Add a file to the store
  $ echo "foo" > foofile
  $ mkdir bar
  $ echo "bar" > bar/file
  $ hg add foofile bar/file
  $ hg commit -m "add some files"
  $ hg push
  pushing to $TESTTMP/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files

# Call this state a base revision
  $ BASEREV="$(hg id -i)"
  $ echo "$BASEREV"
  3490593cf53c

# Snapshot test plan:
# 1) Empty snapshot (no changes);
# 2) Snapshot with an empty metadata (changes only in tracked files);
# 3) Snapshot with metadata (untracked changes only);
# 4) Snapshot with metadata (merge state only);
# 5) Snapshot with metadata (merge state + mixed changes);
# 6) List the snapshots;
# 7) Same as 3 but test the --clean flag on creation;
# 8) Same as 3 but test the --force flag on restore;
# 9) Resolve the merge conflict after restoring the snapshot;
# 10) Negative tests.


# 1) Empty snapshot -- no need to create a snapshot now
  $ hg snapshot create
  nothing changed

  $ hg snapshot list
  no snapshots created


# 2) Snapshot with an empty metadata (changes only in tracked files)
  $ hg rm bar/file
  $ echo "change" >> foofile
  $ echo "another" > bazfile
  $ hg add bazfile
  $ BEFORESTATUS="$(hg status --verbose)"
  $ echo "$BEFORESTATUS"
  M foofile
  A bazfile
  R bar/file
  $ BEFOREDIFF="$(hg diff)"
  $ echo "$BEFOREDIFF"
  diff -r 3490593cf53c bar/file
  --- a/bar/file	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -bar
  diff -r 3490593cf53c bazfile
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/bazfile	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +another
  diff -r 3490593cf53c foofile
  --- a/foofile	Thu Jan 01 00:00:00 1970 +0000
  +++ b/foofile	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   foo
  +change
# Create a snapshot and check the result
  $ EMPTYOID="$(hg snapshot create -m "first snapshot" | head -n 1 | cut -f2 -d' ')"
  $ echo "$EMPTYOID"
  bd8d77aecb3d474ec545981fe5b7aa9cd40f5df2
  $ hg log --hidden -r "$EMPTYOID" -T '{extras % \"{extra}\n\"}' | grep snapshotmetadataid
  snapshotmetadataid=
# The snapshot commit is hidden
  $ hg log --hidden -r  "hidden() & $EMPTYOID"
  changeset:   1:bd8d77aecb3d
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     first snapshot
  
# Rollback to BASEREV and checkout on EMPTYOID
  $ hg update -q --clean "$BASEREV" && rm bazfile
  $ hg status --verbose
  $ hg snapshot checkout "$EMPTYOID"
  will checkout on bd8d77aecb3d474ec545981fe5b7aa9cd40f5df2
  checkout complete
  $ test "$BEFORESTATUS" = "$(hg status --verbose)"
  $ test "$BEFOREDIFF" = "$(hg diff)"


# 3) Snapshot with metadata (untracked changes only);
  $ hg update -q --clean "$BASEREV" && rm bazfile
  $ cd bar
  $ mkdir baz
  $ echo 'a' > untrackedfile
  $ echo 'b' > baz/file
  $ BEFORESTATUS="$(hg status --verbose)"
  $ echo "$BEFORESTATUS"
  ? bar/baz/file
  ? bar/untrackedfile
  $ BEFOREDIFF="$(hg diff)"
  $ echo "$BEFOREDIFF"
  
  $ OIDUNTRACKED="$(hg snapshot create --clean | head -n 1 | cut -f2 -d' ')"
  $ hg snapshot checkout "$OIDUNTRACKED"
  will checkout on e6ce6b866bac5d4f9eaa19abb957c31c0a8957dc
  checkout complete
  $ test "$BEFORESTATUS" = "$(hg status --verbose)"
  $ test "$BEFOREDIFF" = "$(hg diff)"
  $ cd ..


# 4) Snapshot with metadata (merge state only);
  $ hg update -q --clean "$BASEREV" && rm bar/untrackedfile bar/baz/file
  $ hg status --verbose
  $ echo "a" > mergefile
  $ hg add mergefile
  $ hg commit -m "merge #1"
  $ MERGEREV="$(hg id -i)"
  $ hg checkout "$BASEREV"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "b" > mergefile
  $ hg add mergefile
  $ hg commit -m "merge #2"
  $ hg merge "$MERGEREV"
  merging mergefile
  warning: 1 conflicts while merging mergefile! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ BEFORESTATUS="$(hg status --verbose)"
  $ echo "$BEFORESTATUS"
  M mergefile
  ? mergefile.orig
  # The repository is in an unfinished *merge* state.
  
  # Unresolved merge conflicts:
  # 
  #     mergefile
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  
  # To continue:                hg commit
  # To abort:                   hg update --clean .    (warning: this will discard uncommitted changes)
  $ BEFOREDIFF="$(hg diff)"
  $ echo "$BEFOREDIFF"
  diff -r 6eb2552aed20 mergefile
  --- a/mergefile	Thu Jan 01 00:00:00 1970 +0000
  +++ b/mergefile	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,5 @@
  +<<<<<<< working copy: 6eb2552aed20 - test: merge #2
   b
  +=======
  +a
  +>>>>>>> merge rev:    f473d4d5a1c0 - test: merge #1
# Create the snapshot
  $ OID="$(hg snapshot create -m another | cut -f2 -d' ')"
  $ echo "$OID"
  6f770bad8ca50f23f9a62a76c3f7add22772df50

# Clean everything and checkout back
  $ hg update -q --clean . && rm mergefile.orig
  $ hg snapshot checkout "$OID"
  will checkout on 6f770bad8ca50f23f9a62a76c3f7add22772df50
  checkout complete

# hg status/diff are unchanged
  $ test "$BEFORESTATUS" = "$(hg status --verbose)"
  $ test "$BEFOREDIFF" = "$(hg diff)"


# 5) Snapshot with metadata (merge state + mixed changes);
  $ hg status --verbose
  M mergefile
  ? mergefile.orig
  # The repository is in an unfinished *merge* state.
  
  # Unresolved merge conflicts:
  # 
  #     mergefile
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  
  # To continue:                hg commit
  # To abort:                   hg update --clean .    (warning: this will discard uncommitted changes)
  

# Make some changes on top of that: add, remove, edit
  $ hg rm bar/file
  $ rm foofile
  $ echo "another" > bazfile
  $ hg add bazfile
  $ echo "fizz some long content which has a length of at least 100 bytes............................................" > untrackedfile
  $ BEFORESTATUS="$(hg status --verbose)"
  $ echo "$BEFORESTATUS"
  M mergefile
  A bazfile
  R bar/file
  ! foofile
  ? mergefile.orig
  ? untrackedfile
  # The repository is in an unfinished *merge* state.
  
  # Unresolved merge conflicts:
  # 
  #     mergefile
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  
  # To continue:                hg commit
  # To abort:                   hg update --clean .    (warning: this will discard uncommitted changes)
  $ BEFOREDIFF="$(hg diff)"
  $ echo "$BEFOREDIFF"
  diff -r 6eb2552aed20 bar/file
  --- a/bar/file	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -bar
  diff -r 6eb2552aed20 bazfile
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/bazfile	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +another
  diff -r 6eb2552aed20 mergefile
  --- a/mergefile	Thu Jan 01 00:00:00 1970 +0000
  +++ b/mergefile	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,5 @@
  +<<<<<<< working copy: 6eb2552aed20 - test: merge #2
   b
  +=======
  +a
  +>>>>>>> merge rev:    f473d4d5a1c0 - test: merge #1

# Create the snapshot
  $ OID="$(hg snapshot create --clean | head -n 1 | cut -f2 -d' ')"
  $ echo "$OID"
  eae93e849afe8c057f1832a2266720c1c530400e

  $ hg snapshot checkout "$OID"
  will checkout on eae93e849afe8c057f1832a2266720c1c530400e
  checkout complete

# hg status/diff are unchanged
  $ test "$BEFORESTATUS" = "$(hg status --verbose)"
  $ test "$BEFOREDIFF" = "$(hg diff)"

# Check the metadata id and its contents
  $ METADATAID="$(hg log --hidden -r \"$OID\" -T '{extras % \"{extra}\n\"}' | grep snapshotmetadataid | cut -d'=' -f2)"
  $ echo "$METADATAID"
  f37947cc08b744e8892654756add6f883e22616ac835f3da1745cdb6335126b8
  $ find .hg/store/snapshots/objects/"${METADATAID:0:2}"/"${METADATAID:2}"
  .hg/store/snapshots/objects/f3/7947cc08b744e8892654756add6f883e22616ac835f3da1745cdb6335126b8


# 6) List the snapshots
# Check the list of snapshots directly
  $ cat .hg/store/snapshotlist
  v1
  bd8d77aecb3d474ec545981fe5b7aa9cd40f5df2
  e6ce6b866bac5d4f9eaa19abb957c31c0a8957dc
  6f770bad8ca50f23f9a62a76c3f7add22772df50
  eae93e849afe8c057f1832a2266720c1c530400e

# Use the list cmd
  $ hg snapshot list --verbose
  bd8d77aecb3d           None first snapshot
  e6ce6b866bac   937ff3506fea snapshot
  6f770bad8ca5   e654b3eb8739 another
  eae93e849afe   f37947cc08b7 snapshot


# Move back to BASEREV
  $ hg update -q --clean "$BASEREV" && rm bazfile
  $ rm mergefile.orig
  $ hg status
  ? untrackedfile

# Check out on the snapshot -- negative tests
# Regular checkout
  $ hg checkout --hidden "$OID"
  abort: eae93e849afe is a snapshot, set ui.allow-checkout-snapshot config to True to checkout on it
  
  [255]
# Non-empty WC state
  $ hg snapshot checkout "$OID"
  abort: You must have a clean working copy to checkout on a snapshot. Use --force to bypass that.
  
  [255]
# Bad id
  $ rm untrackedfile
  $ hg snapshot checkout somebadid
  somebadid is not a valid revision id
  abort: unknown revision 'somebadid'!
  (if somebadid is a remote bookmark or commit, try to 'hg pull' it first)
  [255]
# Still bad id -- not a snapshot
  $ hg snapshot checkout "$BASEREV"
  abort: 3490593cf53c is not a valid snapshot id
  
  [255]
# Check out on the snapshot -- positive tests
  $ hg snapshot checkout "$OID"
  will checkout on eae93e849afe8c057f1832a2266720c1c530400e
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  checkout complete
  $ test "$BEFORESTATUS" = "$(hg status --verbose)"
  $ test "$BEFOREDIFF" = "$(hg diff)"


# 7) Test the --clean flag on snapshot creation;
  $ find .hg/merge | sort
  .hg/merge
  .hg/merge/fc4ffdcb8ed23cecd44a0e11d23af83b445179b4
  .hg/merge/state
  .hg/merge/state2
  $ hg snapshot create --clean
  snapshot eae93e849afe8c057f1832a2266720c1c530400e created
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status --verbose
  $ test -d .hg/merge
  [1]
  $ hg snapshot checkout "$OID"
  will checkout on eae93e849afe8c057f1832a2266720c1c530400e
  checkout complete


# 8) Test the --force flag on checkout;
  $ hg update -q --clean "$BASEREV"
  $ hg status --verbose
  ? bazfile
  ? mergefile.orig
  ? untrackedfile
  $ hg snapshot checkout "$OID"
  abort: You must have a clean working copy to checkout on a snapshot. Use --force to bypass that.
  
  [255]
  $ hg snapshot checkout --force "$OID"
  will checkout on eae93e849afe8c057f1832a2266720c1c530400e
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  checkout complete


# 9) Resolve the merge conflict after restoring the snapshot;
  $ hg resolve --mark mergefile
  (no more unresolved files)
  $ hg remove foofile
  $ hg status --verbose
  M mergefile
  A bazfile
  R bar/file
  R foofile
  ? mergefile.orig
  ? untrackedfile
  # The repository is in an unfinished *merge* state.
  
  # No unresolved merge conflicts.
  
  # To continue:                hg commit
  # To abort:                   hg update --clean .    (warning: this will discard uncommitted changes)
  
  $ hg commit -m "merge commit"
  $ hg status --verbose
  ? mergefile.orig
  ? untrackedfile


# 10) Negative tests.
  $ BROKENSNAPSHOTID="$(hg snapshot create --clean | head -n 1 | cut -f2 -d' ')"
  $ BROKENMETADATAID="$(hg log --hidden -r \"$BROKENSNAPSHOTID\" -T '{extras % \"{extra}\n\"}' | grep snapshotmetadataid | cut -d'=' -f2)"
# Delete all the related files from the local store
  $ find .hg/store/snapshots/objects/ -mindepth 1 ! -name "${BROKENMETADATAID:2}" -type f -delete
  $ hg snapshot checkout $BROKENSNAPSHOTID
  will checkout on d208b836260dfe6553aa21e569e5451a27e3e599
  abort: file untrackedfile with oid 7c508c43b8899c7e35a808490880699447929ee5e7ee2f279bc22ef8a57dd9e1 not found in local blobstorage
  
  [255]
  $ hg status --verbose
# Break the metadata itself
  $ echo "break the metadata" >> .hg/store/snapshots/objects/"${BROKENMETADATAID:0:2}"/"${BROKENMETADATAID:2}"
  $ hg snapshot checkout $BROKENSNAPSHOTID
  will checkout on d208b836260dfe6553aa21e569e5451a27e3e599
  abort: invalid metadata stream
  
  [255]
  $ hg status --verbose

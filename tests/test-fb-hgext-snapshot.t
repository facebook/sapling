# Initial setup
  $ setconfig extensions.lfs=
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

# No need to create snapshot now
  $ hg debugsnapshot
  nothing changed

# Snapshot with empty manifest (changes only in tracked files)
  $ hg rm bar/file
  $ EMPTYOID="$(hg debugsnapshot | cut -f2 -d' ')"
  $ hg log --hidden -r "$EMPTYOID" -T '{extras % \"{extra}\n\"}' | grep snapshotmanifestid
  snapshotmanifestid=None

# Merge conflict!
  $ hg revert bar/file
  $ echo "a" > mergefile
  $ hg add mergefile
  $ hg commit -m "merge #1"
  $ hg checkout 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "b" > mergefile
  $ hg add mergefile
  $ hg commit -m "merge #2"
  $ hg merge 2
  merging mergefile
  warning: 1 conflicts while merging mergefile! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

# Make some changes on top of that: add, remove, edit
  $ echo "fizz" > untrackedfile
  $ echo "ziff" > bar/untracked
  $ rm foofile
  $ echo "baz" >> bar/file
  $ hg status --verbose
  M bar/file
  M mergefile
  ! foofile
  ? bar/untracked
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
  

# Create the snapshot
  $ OID="$(hg debugsnapshot | cut -f2 -d' ')"
  $ echo "$OID"
  ccdff83036b6b05c657a1eebff7dc523b865f6ce

# Examine the resulting repo state
  $ hg status --verbose
  M bar/file
  M mergefile
  ! foofile
  ? bar/untracked
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
  

# The commit itself is invisible
  $ hg log --hidden -r  "not hidden() & $OID"

# But it exists
  $ hg show --hidden "$OID"
  changeset:   4:ccdff83036b6
  tag:         tip
  parent:      3:6eb2552aed20
  parent:      2:f473d4d5a1c0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/file mergefile
  description:
  snapshot
  
  
  diff -r 6eb2552aed20 -r ccdff83036b6 bar/file
  --- a/bar/file	Thu Jan 01 00:00:00 1970 +0000
  +++ b/bar/file	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   bar
  +baz
  diff -r 6eb2552aed20 -r ccdff83036b6 mergefile
  --- a/mergefile	Thu Jan 01 00:00:00 1970 +0000
  +++ b/mergefile	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,5 @@
  +<<<<<<< working copy: 6eb2552aed20 - test: merge #2
   b
  +=======
  +a
  +>>>>>>> merge rev:    f473d4d5a1c0 - test: merge #1
  

  $ MANIFESTOID="$(hg log --hidden -r \"$OID\" -T '{extras % \"{extra}\n\"}' | tail -1 | cut -d'=' -f2)"
  $ echo "$MANIFESTOID"
  56ccbe0277d166a33f249ffec610dd4776498f55f7d7739a8135051a5bd7c79d

# Check the contents of the manifest file
  $ cat .hg/store/lfs/objects/"${MANIFESTOID:0:2}"/"${MANIFESTOID:2}"
  {"deleted": {"foofile": null}, "localvfsfiles": {"merge/fc4ffdcb8ed23cecd44a0e11d23af83b445179b4": {"oid": "0263829989b6fd954f72baaf2fc64bc2e2f01d692d4de72986ea808f6e99813f", "size": "2"}, "merge/state": {"oid": "fdfea51dfeeae94bd846473c7bef891823af465d33f48e92ed2556bde6b346cb", "size": "166"}, "merge/state2": {"oid": "0e421047ebcf7d0cada48ddd801304725de33da3c4048ccb258041946cd0e81d", "size": "361"}}, "unknown": {"bar/untracked": {"oid": "3d5dd14152511e8e8be2c12f94655132f57710be390f3c45d58ad8661ee78f27", "size": "5"}, "mergefile.orig": {"oid": "0263829989b6fd954f72baaf2fc64bc2e2f01d692d4de72986ea808f6e99813f", "size": "2"}, "untrackedfile": {"oid": "b05b74c474c1706953bed876a19f146b371ddf51a36474fe0c094922385cc479", "size": "5"}}} (no-eol)

# Drop everything
  $ hg update --clean
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated to "6eb2552aed20: merge #2"
  1 other heads for branch "default"
  $ rm mergefile.orig
  $ hg status
  ? bar/untracked
  ? untrackedfile

# Check out on the snapshot
  $ hg debugcheckoutsnapshot --hidden "$OID"
  abort: You must have a clean working copy to checkout on a snapshot. Use --force to bypass that.
  
  [255]

# Oops!
  $ rm untrackedfile bar/untracked
  $ hg debugcheckoutsnapshot somebadid
  somebadid is not a valid revision id
  abort: unknown revision 'somebadid'!
  (if somebadid is a remote bookmark or commit, try to 'hg pull' it first)
  [255]
# Oops!
  $ hg debugcheckoutsnapshot f473d4d5a1c0
  abort: f473d4d5a1c0 is not a valid snapshot id
  
  [255]
# Oops!
  $ hg debugcheckoutsnapshot --hidden "$OID"
  will checkout on ccdff83036b6b05c657a1eebff7dc523b865f6ce
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  checkout complete
  $ hg status --verbose
  M bar/file
  M mergefile
  R foofile
  ? bar/untracked
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
  
# Finally, resolve the conflict
  $ hg resolve --mark mergefile
  (no more unresolved files)
  $ hg status --verbose
  M bar/file
  M mergefile
  R foofile
  ? bar/untracked
  ? mergefile.orig
  ? untrackedfile
  # The repository is in an unfinished *merge* state.
  
  # No unresolved merge conflicts.
  
  # To continue:                hg commit
  # To abort:                   hg update --clean .    (warning: this will discard uncommitted changes)
  
  $ hg commit -m "merge commit"
  $ hg status --verbose
  ? bar/untracked
  ? mergefile.orig
  ? untrackedfile

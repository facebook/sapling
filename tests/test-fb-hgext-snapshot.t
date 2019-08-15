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
  $ echo "bar" > barfile
  $ hg add foofile barfile
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
  $ hg rm barfile
  $ EMPTYOID="$(hg debugsnapshot | cut -f2 -d' ')"
  $ echo "$EMPTYOID"
  825ae4ad3aa841bdb4ed45b5d608689f1bc9d1b3
  $ hg log --hidden -r "$EMPTYOID" -T '{extras % \"{extra}\n\"}' | grep snapshotmanifestid
  snapshotmanifestid=None

# Merge conflict!
  $ hg revert barfile
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
  $ rm foofile
  $ echo "baz" >> barfile
  $ hg status --verbose
  M barfile
  M mergefile
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
  

# Create the snapshot
  $ OID="$(hg debugsnapshot | cut -f2 -d' ')"
  $ echo "$OID"
  75dd4272716b5316ce1d60d6c451b0f32fa749af

# Examine the resulting repo state
  $ hg status --verbose
  M barfile
  M mergefile
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
  

# The commit itself is invisible
  $ hg log --hidden -r  "not hidden() & $OID"

# But it exists
  $ hg show --hidden "$OID"
  changeset:   4:75dd4272716b
  tag:         tip
  parent:      3:e4654c28458b
  parent:      2:49430fc71cd1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       barfile mergefile
  description:
  snapshot
  
  
  diff -r e4654c28458b -r 75dd4272716b barfile
  --- a/barfile	Thu Jan 01 00:00:00 1970 +0000
  +++ b/barfile	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   bar
  +baz
  diff -r e4654c28458b -r 75dd4272716b mergefile
  --- a/mergefile	Thu Jan 01 00:00:00 1970 +0000
  +++ b/mergefile	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,5 @@
  +<<<<<<< working copy: e4654c28458b - test: merge #2
   b
  +=======
  +a
  +>>>>>>> merge rev:    49430fc71cd1 - test: merge #1
  

  $ MANIFESTOID="$(hg log --hidden -r \"$OID\" -T '{extras % \"{extra}\n\"}' | tail -1 | cut -d'=' -f2)"

# Check the contents of the manifest file
  $ cat .hg/store/lfs/objects/"${MANIFESTOID:0:2}"/"${MANIFESTOID:2}"
  {"deleted": {"foofile": null}, "localvfsfiles": {"merge/fc4ffdcb8ed23cecd44a0e11d23af83b445179b4": {"oid": "0263829989b6fd954f72baaf2fc64bc2e2f01d692d4de72986ea808f6e99813f", "size": "2"}, "merge/state": {"oid": "e90e991d748d9353959e5225d6e85ebb7723aaeb7fef5d7c276c38c282a8a996", "size": "166"}, "merge/state2": {"oid": "e86a25de18c4f6428242d0f64e30b4ec458f2d148c9f23319631d0e9859f87f2", "size": "361"}}, "unknown": {"mergefile.orig": {"oid": "0263829989b6fd954f72baaf2fc64bc2e2f01d692d4de72986ea808f6e99813f", "size": "2"}, "untrackedfile": {"oid": "b05b74c474c1706953bed876a19f146b371ddf51a36474fe0c094922385cc479", "size": "5"}}} (no-eol)

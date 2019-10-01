# Initial setup
  $ setconfig extensions.rebase=
  $ setconfig extensions.snapshot=
  $ setconfig extensions.treemanifest=!
  $ setconfig visibility.enabled=true
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon
  $ setconfig infinitepushbackup.logdir="$TESTTMP/logs" infinitepushbackup.hostname=testhost
  $ setconfig snapshot.enable-sync-bundle=true

# Setup server
  $ hg init server
  $ cd server
  $ setupserver
  $ cd ..

# Setup clients
  $ hg clone -q ssh://user@dummy/server client
  $ hg clone -q ssh://user@dummy/server restored
  $ cd client
  $ hg debugvisibility start

# Add a file to the store
  $ echo "foo" > foofile
  $ mkdir bar
  $ echo "bar" > bar/file
  $ hg add foofile bar/file
  $ hg commit -m "add some files"
  $ hg push
  pushing to ssh://user@dummy/server
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 2 changes to 2 files

# Call this state a base revision
  $ BASEREV="$(hg id -i)"
  $ echo "$BASEREV"
  3490593cf53c


# Snapshot backup test plan:
# 1) Create a snapshot, back it up + restore on another client


# 1) Create a snapshot, back it up + restore on another client
# Setup the environment
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
  $ hg rm bar/file
  $ rm foofile
  $ echo "another" > bazfile
  $ hg add bazfile
  $ echo "fizz" > untrackedfile
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

# Make a snapshot
  $ OID="$(hg snapshot create | cut -f2 -d' ')"
  $ echo "$OID"
  f890179e6e66eeb2a5a676efb96f150133a437c9
# Do it one more time to trigger rebundling on the server side
  $ hg snapshot create -m "second snapshot"
  snapshot 1357825b79d89e00627c3c847951eebfd46b6356 created

# Back it up
  $ hg cloud backup
  backing up stack rooted at f473d4d5a1c0
  remote: pushing 4 commits:
  remote:     f473d4d5a1c0  merge #1
  remote:     6eb2552aed20  merge #2
  remote:     f890179e6e66  snapshot
  remote:     1357825b79d8  second snapshot
  backing up stack rooted at 6eb2552aed20
  remote: pushing 4 commits:
  remote:     f473d4d5a1c0  merge #1
  remote:     6eb2552aed20  merge #2
  remote:     f890179e6e66  snapshot
  remote:     1357825b79d8  second snapshot
  commitcloud: backed up 4 commits

# Restore it on another client
  $ cd ../restored
  $ hg pull -r "$OID"
  pulling from ssh://user@dummy/server
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 4 changes to 2 files
  new changesets 3490593cf53c:f890179e6e66
  $ hg snapshot checkout "$OID"
  will checkout on f890179e6e66eeb2a5a676efb96f150133a437c9
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  checkout complete
# hg status/diff are unchanged
  $ test "$BEFORESTATUS" = "$(hg status --verbose)"
  $ test "$BEFOREDIFF" = "$(hg diff)"
# The snapshot commit is hidden
  $ hg show "$OID"
  abort: hidden revision 'f890179e6e66eeb2a5a676efb96f150133a437c9'!
  (use --hidden to access hidden revisions)
  [255]

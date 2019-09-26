# Initial setup
  $ setconfig extensions.amend=
  $ setconfig extensions.rebase=
  $ setconfig extensions.snapshot=
  $ setconfig extensions.smartlog=
  $ setconfig extensions.treemanifest=!
  $ setconfig smartlog.hide-before="0 0"
  $ setconfig visibility.enabled=true

  $ mkcommit()
  > {
  >   echo "$1" > "$1"
  >   hg add "$1"
  >   hg commit -m "$1"
  > }

# Prepare server and client repos.
  $ hg init server
  $ hg clone -q server client
  $ cd client
  $ hg debugvisibility start

# Add some files to the store
  $ mkcommit root
  $ mkcommit public1
  $ hg phase -p .
  $ echo "foo" > foofile
  $ mkdir bar
  $ echo "bar" > bar/file
  $ hg add foofile bar/file
  $ hg commit -m "add some files"

# Call this state a base revision
  $ BASEREV="$(hg id -i)"
  $ echo "$BASEREV"
  fa948fa73a59

# Add some hidden commits
  $ mkcommit draft1
  $ hg amend -m "draft1 amend1"
  $ hg amend -m "draft1 amend2"
  $ mkcommit draft2


# Snapshot show test plan:
# 1) Create a couple of snapshots (with public and with hidden parents);
# 2) Show these snapshots;
# 3) Show them in ssl;
# 4) Hide a snapshot, check the ssl and unhide it;


# 1) Create a couple of snapshots (with public and with hidden parents);
  $ echo "a" > mergefile
  $ hg add mergefile
  $ hg commit -m "merge #1"
  $ MERGEREV="$(hg id -i)"
  $ hg checkout "$BASEREV"
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ echo "b" > mergefile
  $ hg add mergefile
  $ hg commit -m "merge #2"
  $ hg merge "$MERGEREV"
  merging mergefile
  warning: 1 conflicts while merging mergefile! (edit, then use 'hg resolve --mark')
  2 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

# Make some changes on top of that: add, remove, edit
  $ hg rm bar/file
  $ rm foofile
  $ echo "another" > bazfile
  $ hg add bazfile
  $ echo "fizz" > untrackedfile

# Create the snapshot
  $ OID="$(hg snapshot create --clean | head -n 1 | cut -f2 -d' ')"
  $ echo "$OID"
  b6124cfe90c6a103b62a83944bc2dfc2435f539a

# And another one, on the top of a hidden commit
  $ hg checkout --hidden 3
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'a' > a
  $ HOID="$(hg snapshot create --clean | head -n 1 | cut -f2 -d' ')"
  $ echo "$HOID"
  3d1b299b75fb94d133a1199843576653a7634e48

  $ hg checkout "$BASEREV"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved


# 2) Show these snapshots;
  $ hg snapshot show "$BASEREV"
  abort: fa948fa73a59 is not a valid snapshot id
  
  [255]
  $ hg snapshot show "$OID"
  changeset:   9:b6124cfe90c6
  parent:      8:fdf2c0326bba
  parent:      7:9d3ebf4630d3
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     snapshot
  
  diff -r fdf2c0326bba -r b6124cfe90c6 bar/file
  --- a/bar/file	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -bar
  diff -r fdf2c0326bba -r b6124cfe90c6 bazfile
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/bazfile	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +another
  diff -r fdf2c0326bba -r b6124cfe90c6 draft1
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/draft1	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +draft1
  diff -r fdf2c0326bba -r b6124cfe90c6 draft2
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/draft2	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +draft2
  diff -r fdf2c0326bba -r b6124cfe90c6 mergefile
  --- a/mergefile	Thu Jan 01 00:00:00 1970 +0000
  +++ b/mergefile	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,5 @@
  +<<<<<<< working copy: fdf2c0326bba - test: merge #2
   b
  +=======
  +a
  +>>>>>>> merge rev:    9d3ebf4630d3 - test: merge #1
  
  ===
  Untracked changes:
  ===
  ? mergefile.orig
  @@ -0,0 +1,1 @@
  +b
  ? untrackedfile
  @@ -0,0 +1,1 @@
  +fizz
  ! foofile
  @@ -1,1 +0,0 @@
  -foo
  
  The snapshot is in an unfinished *merge* state.

  $ hg snapshot show "$HOID"
  changeset:   10:3d1b299b75fb
  tag:         tip
  parent:      3:ffb8db6e9ac3
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  instability: orphan
  summary:     snapshot
  
  
  ===
  Untracked changes:
  ===
  ? a
  @@ -0,0 +1,1 @@
  +a
  

  $ hg show --hidden "$HOID"
  changeset:   10:3d1b299b75fb
  tag:         tip
  parent:      3:ffb8db6e9ac3
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  instability: orphan
  description:
  snapshot
  
  
  

# 3) Show them in ssl
  $ hg smartlog -T default
  s    changeset:   9:b6124cfe90c6
  |\   parent:      8:fdf2c0326bba
  | |  parent:      7:9d3ebf4630d3
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     snapshot
  | |
  | o  changeset:   8:fdf2c0326bba
  | |  parent:      2:fa948fa73a59
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     merge #2
  | |
  o |  changeset:   7:9d3ebf4630d3
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     merge #1
  | |
  o |  changeset:   6:8e676f2ef130
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     draft2
  | |
  o |  changeset:   5:d521223a2fb5
  |/   parent:      2:fa948fa73a59
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     draft1 amend2
  |
  | s  changeset:   10:3d1b299b75fb
  | |  tag:         tip
  | |  parent:      3:ffb8db6e9ac3
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  instability: orphan
  | |  summary:     snapshot
  | |
  | x  changeset:   3:ffb8db6e9ac3
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    obsolete:    rewritten using amend as 5:d521223a2fb5
  |    summary:     draft1
  |
  @  changeset:   2:fa948fa73a59
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add some files
  |
  o  changeset:   1:175dbab47dcc
  |  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     public1
  


# If we don't have a snapshot extension
# TODO(alexeyqu): figure out why we show 3 here
  $ setconfig extensions.snapshot=!
  $ hg smartlog -T default
  o  changeset:   8:fdf2c0326bba
  |  parent:      2:fa948fa73a59
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     merge #2
  |
  | o  changeset:   7:9d3ebf4630d3
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     merge #1
  | |
  | o  changeset:   6:8e676f2ef130
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     draft2
  | |
  | o  changeset:   5:d521223a2fb5
  |/   parent:      2:fa948fa73a59
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     draft1 amend2
  |
  | x  changeset:   3:ffb8db6e9ac3
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    obsolete:    rewritten using amend as 5:d521223a2fb5
  |    summary:     draft1
  |
  @  changeset:   2:fa948fa73a59
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add some files
  |
  o  changeset:   1:175dbab47dcc
  |  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     public1
  
  $ setconfig extensions.snapshot=


# 4) Hide a snapshot, check the ssl and unhide it;
  $ hg snapshot hide "$OID"
  $ hg snapshot list
  3d1b299b75fb snapshot
  $ hg smartlog -T default
  o  changeset:   8:fdf2c0326bba
  |  parent:      2:fa948fa73a59
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     merge #2
  |
  | o  changeset:   7:9d3ebf4630d3
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     merge #1
  | |
  | o  changeset:   6:8e676f2ef130
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     draft2
  | |
  | o  changeset:   5:d521223a2fb5
  |/   parent:      2:fa948fa73a59
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     draft1 amend2
  |
  | s  changeset:   10:3d1b299b75fb
  | |  tag:         tip
  | |  parent:      3:ffb8db6e9ac3
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  instability: orphan
  | |  summary:     snapshot
  | |
  | x  changeset:   3:ffb8db6e9ac3
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    obsolete:    rewritten using amend as 5:d521223a2fb5
  |    summary:     draft1
  |
  @  changeset:   2:fa948fa73a59
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add some files
  |
  o  changeset:   1:175dbab47dcc
  |  user:        test
  ~  date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     public1
  
  $ hg snapshot unhide "$OID"
  $ hg snapshot list
  3d1b299b75fb snapshot
  b6124cfe90c6 snapshot
  $ hg unhide "$HOID"
  $ hg log -r "snapshot() & hidden()" --hidden
  changeset:   9:b6124cfe90c6
  parent:      8:fdf2c0326bba
  parent:      7:9d3ebf4630d3
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     snapshot
  
  changeset:   10:3d1b299b75fb
  tag:         tip
  parent:      3:ffb8db6e9ac3
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  instability: orphan
  summary:     snapshot
  

#chg-compatible

TODO: configure mutation
  $ configure noevolution
  $ disable treemanifest
  $ . helpers-usechg.sh

  $ setconfig format.usegeneraldelta=yes

  $ restore() {
  >     hg unbundle -q .hg/strip-backup/*
  >     rm .hg/strip-backup/*
  > }
  $ teststrip() {
  >     hg up -C $1
  >     echo % before update $1, strip $2
  >     hg parents
  >     hg --traceback debugstrip $2
  >     echo % after update $1, strip $2
  >     hg parents
  >     restore
  > }

  $ hg init test
  $ cd test

  $ echo foo > bar
  $ hg ci -Ama
  adding bar

  $ echo more >> bar
  $ hg ci -Amb

  $ echo blah >> bar
  $ hg ci -Amc

  $ hg up 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo blah >> bar
  $ hg ci -Amd

  $ echo final >> bar
  $ hg ci -Ame

  $ hg log
  changeset:   4:443431ffac4f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  
  changeset:   3:65bd5f99a4a3
  parent:      1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     d
  
  changeset:   2:264128213d29
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
  changeset:   1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   0:9ab35a2d17cb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  

  $ teststrip 4 4
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % before update 4, strip 4
  changeset:   4:443431ffac4f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  % after update 4, strip 4
  changeset:   3:65bd5f99a4a3
  parent:      1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     d
  
  $ teststrip 4 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % before update 4, strip 3
  changeset:   4:443431ffac4f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  % after update 4, strip 3
  changeset:   1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  $ teststrip 1 4
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % before update 1, strip 4
  changeset:   1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  % after update 1, strip 4
  changeset:   1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  $ teststrip 4 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % before update 4, strip 2
  changeset:   4:443431ffac4f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  % after update 4, strip 2
  changeset:   3:443431ffac4f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  
  $ teststrip 4 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % before update 4, strip 1
  changeset:   4:264128213d29
  parent:      1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  % after update 4, strip 1
  changeset:   0:9ab35a2d17cb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ teststrip null 4
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  % before update null, strip 4
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  % after update null, strip 4

  $ hg log
  changeset:   4:264128213d29
  parent:      1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
  changeset:   3:443431ffac4f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  
  changeset:   2:65bd5f99a4a3
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     d
  
  changeset:   1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   0:9ab35a2d17cb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ hg up -C 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg parents
  changeset:   4:264128213d29
  parent:      1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  

  $ hg --traceback debugstrip 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/264128213d29-0b39d6bf-backup.hg
  $ hg parents
  changeset:   1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  $ hg debugbundle .hg/strip-backup/*
  Stream params: {Compression: BZ}
  changegroup -- {nbchanges: 1, version: 02}
      264128213d290d868c54642d13aeaa3675551a78
  phase-heads -- {}
      264128213d290d868c54642d13aeaa3675551a78 draft
  $ hg pull .hg/strip-backup/*
  pulling from .hg/strip-backup/264128213d29-0b39d6bf-backup.hg
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  $ rm .hg/strip-backup/*
  $ hg log --graph
  o  changeset:   4:264128213d29
  |  parent:      1:ef3a871183d7
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  | o  changeset:   3:443431ffac4f
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     e
  | |
  | o  changeset:   2:65bd5f99a4a3
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     d
  |
  @  changeset:   1:ef3a871183d7
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:9ab35a2d17cb
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
  $ hg up -C 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge 4
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

before strip of merge parent

  $ hg parents
  changeset:   2:65bd5f99a4a3
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     d
  
  changeset:   4:264128213d29
  parent:      1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
  $ hg debugstrip 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)

after strip of merge parent

  $ hg parents
  changeset:   1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  $ restore

  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated to "264128213d29: c"
  1 other heads for branch "default"
  $ hg log -G
  @  changeset:   4:264128213d29
  |  parent:      1:ef3a871183d7
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  | o  changeset:   3:443431ffac4f
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     e
  | |
  | o  changeset:   2:65bd5f99a4a3
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     d
  |
  o  changeset:   1:ef3a871183d7
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:9ab35a2d17cb
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  

2 is parent of 3, only one strip should happen

  $ hg debugstrip "roots(2)" 3
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  $ hg log -G
  @  changeset:   2:264128213d29
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   1:ef3a871183d7
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:9ab35a2d17cb
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
  $ restore
  $ hg log -G
  o  changeset:   4:443431ffac4f
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   3:65bd5f99a4a3
  |  parent:      1:ef3a871183d7
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  | @  changeset:   2:264128213d29
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     c
  |
  o  changeset:   1:ef3a871183d7
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:9ab35a2d17cb
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
Failed hook while applying "saveheads" bundle.

  $ hg debugstrip 2 --config hooks.pretxnchangegroup.bad=false
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  transaction abort!
  rollback completed
  strip failed, backup bundle stored in '$TESTTMP/test/.hg/strip-backup/*-backup.hg' (glob)
  strip failed, unrecovered changes stored in '$TESTTMP/test/.hg/strip-backup/*-temp.hg' (glob)
  (fix the problem, then recover the changesets with "hg unbundle '$TESTTMP/test/.hg/strip-backup/*-temp.hg'") (glob)
  abort: pretxnchangegroup.bad hook exited with status 1
  [255]
  $ restore
  $ hg log -G
  o  changeset:   4:443431ffac4f
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   3:65bd5f99a4a3
  |  parent:      1:ef3a871183d7
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  | o  changeset:   2:264128213d29
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     c
  |
  @  changeset:   1:ef3a871183d7
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:9ab35a2d17cb
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  

2 different branches: 2 strips

  $ hg debugstrip 2 4
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  $ hg log -G
  o  changeset:   2:65bd5f99a4a3
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  @  changeset:   1:ef3a871183d7
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:9ab35a2d17cb
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
  $ restore

2 different branches and a common ancestor: 1 strip

  $ hg debugstrip 1 "2|4"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  $ restore

verify fncache is kept up-to-date

  $ touch a
  $ hg ci -qAm a
  $ cat .hg/store/fncache | sort
  data/a.i
  data/bar.i
  $ hg debugstrip tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  $ cat .hg/store/fncache
  data/bar.i

stripping an empty revset

  $ hg debugstrip "1 and not 1"
  abort: empty revision set
  [255]

Strip adds, removes, modifies with --keep

  $ touch b
  $ hg add b
  $ hg commit -mb
  $ touch c

... with a clean working dir

  $ hg add c
  $ hg rm bar
  $ hg commit -mc
  $ hg status
  $ hg debugstrip --keep tip
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  $ hg status
  ! bar
  ? c

... with a dirty working dir

  $ hg add c
  $ hg rm bar
  $ hg commit -mc
  $ hg status
  $ echo b > b
  $ echo d > d
  $ hg debugstrip --keep tip
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  $ hg status
  M b
  ! bar
  ? c
  ? d

... after updating the dirstate
  $ hg add c
  $ hg commit -mc
  $ hg rm c
  $ hg commit -mc
  $ hg debugstrip --keep '.^' -q
  $ cd ..

stripping many nodes on a complex graph (issue3299)

  $ hg init issue3299
  $ cd issue3299
  $ hg debugbuilddag '@a.:a@b.:b.:x<a@a.:a<b@b.:b<a@a.:a'
  $ hg debugstrip 'not ancestors(x)'
  saved backup bundle to $TESTTMP/issue3299/.hg/strip-backup/*-backup.hg (glob)

test hg debugstrip -B bookmark

  $ cd ..
  $ hg init bookmarks
  $ cd bookmarks
  $ hg debugbuilddag '..<2.*1/2:m<2+3:c<m+3:a<2.:b<m+2:d<2.:e<m+1:f'
  $ hg bookmark -r 'a' 'todelete'
  $ hg bookmark -r 'b' 'B'
  $ hg bookmark -r 'b' 'nostrip'
  $ hg bookmark -r 'c' 'delete'
  $ hg bookmark -r 'd' 'multipledelete1'
  $ hg bookmark -r 'e' 'multipledelete2'
  $ hg bookmark -r 'f' 'singlenode1'
  $ hg bookmark -r 'f' 'singlenode2'
  $ hg book -d a b c d e f m
  $ hg up -C todelete
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark todelete)
  $ hg debugstrip -B nostrip
  bookmark 'nostrip' deleted
  abort: empty revision set
  [255]
  $ hg debugstrip -B todelete
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/bookmarks/.hg/strip-backup/*-backup.hg (glob)
  bookmark 'todelete' deleted
  $ hg id -ir dcbb326fdec2
  abort: unknown revision 'dcbb326fdec2'!
  (if dcbb326fdec2 is a remote bookmark or commit, try to 'hg pull' it first)
  [255]
  $ hg id -ir d62d843c9a01
  d62d843c9a01
  $ hg bookmarks
     B                         9:ff43616e5d0f
     delete                    6:2702dd0c91e7
     multipledelete1           11:e46a4836065c
     multipledelete2           12:b4594d867745
     singlenode1               13:43227190fef8
     singlenode2               13:43227190fef8
  $ hg debugstrip -B multipledelete1 -B multipledelete2
  saved backup bundle to $TESTTMP/bookmarks/.hg/strip-backup/*-backup.hg (glob)
  bookmark 'multipledelete1' deleted
  bookmark 'multipledelete2' deleted
  $ hg id -ir e46a4836065c
  abort: unknown revision 'e46a4836065c'!
  (if e46a4836065c is a remote bookmark or commit, try to 'hg pull' it first)
  [255]
  $ hg id -ir b4594d867745
  abort: unknown revision 'b4594d867745'!
  (if b4594d867745 is a remote bookmark or commit, try to 'hg pull' it first)
  [255]
  $ hg debugstrip -B singlenode1 -B singlenode2
  saved backup bundle to $TESTTMP/bookmarks/.hg/strip-backup/*-backup.hg (glob)
  bookmark 'singlenode1' deleted
  bookmark 'singlenode2' deleted
  $ hg id -ir 43227190fef8
  abort: unknown revision '43227190fef8'!
  (if 43227190fef8 is a remote bookmark or commit, try to 'hg pull' it first)
  [255]
  $ hg debugstrip -B unknownbookmark
  abort: bookmark not found: 'unknownbookmark'
  [255]
  $ hg debugstrip -B unknownbookmark1 -B unknownbookmark2
  abort: bookmark not found: 'unknownbookmark1', 'unknownbookmark2'
  [255]
  $ hg debugstrip -B delete -B unknownbookmark
  abort: bookmark not found: 'unknownbookmark'
  [255]
  $ hg debugstrip -B delete
  saved backup bundle to $TESTTMP/bookmarks/.hg/strip-backup/*-backup.hg (glob)
  bookmark 'delete' deleted
  $ hg id -ir 6:2702dd0c91e7
  abort: unknown revision '2702dd0c91e7'!
  (if 2702dd0c91e7 is a remote bookmark or commit, try to 'hg pull' it first)
  [255]
  $ hg update B
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark B)
  $ echo a > a
  $ hg add a
  $ hg debugstrip -B B
  abort: local changes found
  [255]
  $ hg bookmarks
   * B                         6:ff43616e5d0f

Make sure no one adds back a -b option:

  $ hg debugstrip -b tip
  hg debugstrip: option -b not recognized
  (use 'hg debugstrip -h' to get help)
  [255]

  $ cd ..

Verify bundles don't get overwritten:

  $ hg init doublebundle
  $ cd doublebundle
  $ touch a
  $ hg commit -Aqm a
  $ touch b
  $ hg commit -Aqm b
  $ hg debugstrip -r 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/doublebundle/.hg/strip-backup/3903775176ed-e68910bd-backup.hg
  $ ls .hg/strip-backup
  3903775176ed-e68910bd-backup.hg
  $ hg pull -q -r 3903775176ed .hg/strip-backup/3903775176ed-e68910bd-backup.hg
  $ hg debugstrip -r 0
  saved backup bundle to $TESTTMP/doublebundle/.hg/strip-backup/3903775176ed-54390173-backup.hg
  $ ls .hg/strip-backup
  3903775176ed-54390173-backup.hg
  3903775176ed-e68910bd-backup.hg
  $ cd ..

Test that we only bundle the stripped changesets (issue4736)
------------------------------------------------------------

initialization (previous repo is empty anyway)

  $ hg init issue4736
  $ cd issue4736
  $ echo a > a
  $ hg add a
  $ hg commit -m commitA
  $ echo b > b
  $ hg add b
  $ hg commit -m commitB
  $ echo c > c
  $ hg add c
  $ hg commit -m commitC
  $ hg up 'desc(commitB)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo d > d
  $ hg add d
  $ hg commit -m commitD
  $ hg up 'desc(commitC)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge 'desc(commitD)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'mergeCD'
  $ hg log -G
  @    changeset:   4:d8db9d137221
  |\   parent:      2:5c51d8d6557d
  | |  parent:      3:6625a5168474
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     mergeCD
  | |
  | o  changeset:   3:6625a5168474
  | |  parent:      1:eca11cf91c71
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     commitD
  | |
  o |  changeset:   2:5c51d8d6557d
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     commitC
  |
  o  changeset:   1:eca11cf91c71
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     commitB
  |
  o  changeset:   0:105141ef12d0
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     commitA
  

Check bundle behavior:

  $ hg bundle -r 'desc(mergeCD)' --base 'desc(commitC)' ../issue4736.hg
  2 changesets found
  $ hg log -r 'bundle()' -R ../issue4736.hg
  changeset:   3:6625a5168474
  parent:      1:eca11cf91c71
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commitD
  
  changeset:   4:d8db9d137221
  parent:      2:5c51d8d6557d
  parent:      3:6625a5168474
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     mergeCD
  

check strip behavior

  $ hg debugstrip 'desc(commitD)' --debug
  resolving manifests
   branchmerge: False, force: True, partial: False
   ancestor: d8db9d137221+, local: d8db9d137221+, remote: eca11cf91c71
   c: other deleted -> r
  removing c
   d: other deleted -> r
  removing d
  starting 4 threads for background file closing (?)
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  2 changesets found
  list of changesets:
  6625a516847449b6f0fa3737b9ba56e9f0f3032c
  d8db9d1372214336d2b5570f20ee468d2c72fa8b
  bundle2-output-bundle: "HG20", (1 params) 2 parts total
  bundle2-output-part: "changegroup" (params: 1 mandatory 1 advisory) streamed payload
  bundle2-output-part: "phase-heads" 24 bytes payload
  saved backup bundle to $TESTTMP/issue4736/.hg/strip-backup/6625a5168474-345bb43d-backup.hg
  $ hg log -G
  o  changeset:   2:5c51d8d6557d
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     commitC
  |
  @  changeset:   1:eca11cf91c71
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     commitB
  |
  o  changeset:   0:105141ef12d0
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     commitA
  

strip backup content

  $ hg log -r 'bundle()' -R .hg/strip-backup/6625a5168474-*-backup.hg
  changeset:   3:6625a5168474
  parent:      1:eca11cf91c71
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commitD
  
  changeset:   4:d8db9d137221
  parent:      2:5c51d8d6557d
  parent:      3:6625a5168474
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     mergeCD
  
Check that the phase cache is properly invalidated after a strip with bookmark.

  $ cat > ../stripstalephasecache.py << EOF
  > from edenscm.mercurial import extensions, localrepo
  > def transactioncallback(orig, repo, desc, *args, **kwargs):
  >     def test(transaction):
  >         # observe cache inconsistency
  >         try:
  >             [repo.changelog.node(r) for r in repo.revs("not public()")]
  >         except IndexError:
  >             repo.ui.status("Index error!\n")
  >     transaction = orig(repo, desc, *args, **kwargs)
  >     # warm up the phase cache
  >     list(repo.revs("not public()"))
  >     if desc != 'strip':
  >          transaction.addpostclose("phase invalidation test", test)
  >     return transaction
  > def extsetup(ui):
  >     extensions.wrapfunction(localrepo.localrepository, "transaction",
  >                             transactioncallback)
  > EOF
  $ hg up -C 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo k > k
  $ hg add k
  $ hg commit -m commitK
  $ echo l > l
  $ hg add l
  $ hg commit -m commitL
  $ hg book -r tip blah
  $ hg debugstrip ".^" --config extensions.crash=$TESTTMP/stripstalephasecache.py
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/issue4736/.hg/strip-backup/8f0b4384875c-4fa10deb-backup.hg
  $ hg up -C 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Error during post-close callback of the strip transaction
(They should be gracefully handled and reported)

  $ cat > ../crashstrip.py << EOF
  > from edenscm.mercurial import error
  > def reposetup(ui, repo):
  >     class crashstriprepo(repo.__class__):
  >         def transaction(self, desc, *args, **kwargs):
  >             tr = super(crashstriprepo, self).transaction(desc, *args, **kwargs)
  >             if desc == 'strip':
  >                 def crash(tra): raise error.Abort('boom')
  >                 tr.addpostclose('crash', crash)
  >             return tr
  >     repo.__class__ = crashstriprepo
  > EOF
  $ hg debugstrip tip --config extensions.crash=$TESTTMP/crashstrip.py
  saved backup bundle to $TESTTMP/issue4736/.hg/strip-backup/5c51d8d6557d-70daef06-backup.hg
  strip failed, backup bundle stored in '$TESTTMP/issue4736/.hg/strip-backup/5c51d8d6557d-70daef06-backup.hg'
  abort: boom
  [255]

Use delayedstrip to strip inside a transaction

  $ cd $TESTTMP
  $ hg init delayedstrip
  $ cd delayedstrip
  $ hg debugdrawdag <<'EOS'
  >   D
  >   |
  >   C F H    # Commit on top of "I",
  >   | |/|    # Strip B+D+I+E+G+H+Z
  > I B E G
  >  \|/
  >   A   Z
  > EOS
  $ cp -R . ../scmutilcleanup

  $ hg up -C I
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark I)
  $ echo 3 >> I
  $ cat > $TESTTMP/delayedstrip.py <<EOF
  > from __future__ import absolute_import
  > from edenscm.mercurial import commands, registrar, repair
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('testdelayedstrip')
  > def testdelayedstrip(ui, repo):
  >     def getnodes(expr):
  >         return [repo.changelog.node(r) for r in repo.revs(expr)]
  >     with repo.wlock():
  >         with repo.lock():
  >             with repo.transaction('delayedstrip'):
  >                 repair.delayedstrip(ui, repo, getnodes('B+I+Z+D+E'), 'J')
  >                 repair.delayedstrip(ui, repo, getnodes('G+H+Z'), 'I')
  >                 commands.commit(ui, repo, message='J', date='0 0')
  > EOF
  $ hg testdelayedstrip --config extensions.t=$TESTTMP/delayedstrip.py
  warning: orphaned descendants detected, not stripping 08ebfeb61bac, 112478962961, 7fb047a69f22
  saved backup bundle to $TESTTMP/delayedstrip/.hg/strip-backup/f585351a92f8-17475721-I.hg

  $ hg log -G -T '{rev}:{node|short} {desc}' -r 'sort(all(), topo)'
  @  6:2f2d51af6205 J
  |
  o  3:08ebfeb61bac I
  |
  | o  5:64a8289d2492 F
  | |
  | o  2:7fb047a69f22 E
  |/
  | o  4:26805aba1e60 C
  | |
  | o  1:112478962961 B
  |/
  o  0:426bada5c675 A
  
Test high-level scmutil.cleanupnodes API

  $ cd $TESTTMP/scmutilcleanup
  $ hg debugdrawdag <<'EOS'
  >   D2  F2  G2   # D2, F2, G2 are replacements for D, F, G
  >   |   |   |
  >   C   H   G
  > EOS
  $ for i in B C D F G I Z; do
  >     hg bookmark -i -r $i b-$i
  > done
  $ hg bookmark -i -r E 'b-F@divergent1'
  $ hg bookmark -i -r H 'b-F@divergent2'
  $ hg bookmark -i -r G 'b-F@divergent3'
  $ cp -R . ../scmutilcleanup.obsstore

  $ cat > $TESTTMP/scmutilcleanup.py <<EOF
  > from edenscm.mercurial import registrar, scmutil
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('testnodescleanup')
  > def testnodescleanup(ui, repo):
  >     def nodes(expr):
  >         return [repo.changelog.node(r) for r in repo.revs(expr)]
  >     def node(expr):
  >         return nodes(expr)[0]
  >     with repo.wlock():
  >         with repo.lock():
  >             with repo.transaction('delayedstrip'):
  >                 mapping = {node('F'): [node('F2')],
  >                            node('D'): [node('D2')],
  >                            node('G'): [node('G2')]}
  >                 scmutil.cleanupnodes(repo, mapping, 'replace')
  >                 scmutil.cleanupnodes(repo, nodes('((B::)+I+Z)-D2'), 'replace')
  > EOF
  $ hg testnodescleanup --config extensions.t=$TESTTMP/scmutilcleanup.py
  warning: orphaned descendants detected, not stripping 112478962961, 1fc8102cda62, 26805aba1e60
  saved backup bundle to $TESTTMP/scmutilcleanup/.hg/strip-backup/f585351a92f8-73fb7c03-replace.hg

  $ hg log -G -T '{rev}:{node|short} {desc} {bookmarks}' -r 'sort(all(), topo)'
  o  8:1473d4b996d1 G2 G G2 b-F@divergent3 b-G
  |
  | o  7:d11b3456a873 F2 F F2 b-F
  | |
  | o  5:5cb05ba470a7 H H
  |/|
  | o  3:7fb047a69f22 E E b-F@divergent1
  | |
  | | o  6:7c78f703e465 D2 D D2 b-D
  | | |
  | | o  4:26805aba1e60 C
  | | |
  | | o  2:112478962961 B
  | |/
  o |  1:1fc8102cda62 G
   /
  o  0:426bada5c675 A A B C I b-B b-C b-I
  
  $ hg bookmark
     A                         0:426bada5c675
     B                         0:426bada5c675
     C                         0:426bada5c675
     D                         6:7c78f703e465
     D2                        6:7c78f703e465
     E                         3:7fb047a69f22
     F                         7:d11b3456a873
     F2                        7:d11b3456a873
     G                         8:1473d4b996d1
     G2                        8:1473d4b996d1
     H                         5:5cb05ba470a7
     I                         0:426bada5c675
     Z                         -1:000000000000
     b-B                       0:426bada5c675
     b-C                       0:426bada5c675
     b-D                       6:7c78f703e465
     b-F                       7:d11b3456a873
     b-F@divergent1            3:7fb047a69f22
     b-F@divergent3            8:1473d4b996d1
     b-G                       8:1473d4b996d1
     b-I                       0:426bada5c675
     b-Z                       -1:000000000000

Test the above using obsstore "by the way". Not directly related to strip, but
we have reusable code here

  $ cd $TESTTMP/scmutilcleanup.obsstore
  $ cat >> .hg/hgrc <<EOF
  > [experimental]
  > evolution=true
  > evolution.track-operation=1
  > EOF

  $ hg testnodescleanup --config extensions.t=$TESTTMP/scmutilcleanup.py

  $ hg log -G -T '{rev}:{node|short} {desc} {bookmarks}' -r 'sort(all(), topo)'
  o  12:1473d4b996d1 G2 G G2 b-F@divergent3 b-G
  |
  | o  11:d11b3456a873 F2 F F2 b-F
  | |
  | o  8:5cb05ba470a7 H H
  |/|
  | o  4:7fb047a69f22 E E b-F@divergent1
  | |
  | | o  10:7c78f703e465 D2 D D2 b-D
  | | |
  | | x  6:26805aba1e60 C
  | | |
  | | x  3:112478962961 B
  | |/
  x |  1:1fc8102cda62 G
   /
  o  0:426bada5c675 A A B C I b-B b-C b-I
  
  $ hg debugobsolete
  1fc8102cda6204549f031015641606ccf5513ec3 1473d4b996d1d1b121de6b39fab6a04fbf9d873e 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'replace', 'user': 'test'}
  64a8289d249234b9886244d379f15e6b650b28e3 d11b3456a873daec7c7bc53e5622e8df6d741bd2 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'replace', 'user': 'test'}
  f585351a92f85104bff7c284233c338b10eb1df7 7c78f703e465d73102cc8780667ce269c5208a40 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'replace', 'user': 'test'}
  48b9aae0607f43ff110d84e6883c151942add5ab 0 {0000000000000000000000000000000000000000} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'replace', 'user': 'test'}
  112478962961147124edd43549aedd1a335e44bf 0 {426bada5c67598ca65036d57d9e4b64b0c1ce7a0} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'replace', 'user': 'test'}
  08ebfeb61bac6e3f12079de774d285a0d6689eba 0 {426bada5c67598ca65036d57d9e4b64b0c1ce7a0} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'replace', 'user': 'test'}
  26805aba1e600a82e93661149f2313866a221a7b 0 {112478962961147124edd43549aedd1a335e44bf} (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'replace', 'user': 'test'}
  $ cd ..

Test that obsmarkers are restored even when not using generaldelta

  $ hg --config format.usegeneraldelta=no init issue5678
  $ cd issue5678
  $ cat >> .hg/hgrc <<EOF
  > [experimental]
  > evolution=true
  > EOF
  $ echo a > a
  $ hg ci -Aqm a
  $ hg ci --amend -m a2
  $ hg debugobsolete
  cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b 489bac576828490c0bb8d45eac9e5e172e4ec0a8 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'test'}
  $ hg debugstrip .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/issue5678/.hg/strip-backup/489bac576828-bef27e14-backup.hg
  $ hg unbundle -q .hg/strip-backup/*
  $ hg debugobsolete
  cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b 489bac576828490c0bb8d45eac9e5e172e4ec0a8 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'test'}
  $ cd ..

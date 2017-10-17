  $ echo "[format]" >> $HGRCPATH
  $ echo "usegeneraldelta=yes" >> $HGRCPATH
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "strip=" >> $HGRCPATH
  $ echo "drawdag=$TESTDIR/drawdag.py" >> $HGRCPATH

  $ restore() {
  >     hg unbundle -q .hg/strip-backup/*
  >     rm .hg/strip-backup/*
  > }
  $ teststrip() {
  >     hg up -C $1
  >     echo % before update $1, strip $2
  >     hg parents
  >     hg --traceback strip $2
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
  created new head

  $ echo final >> bar
  $ hg ci -Ame

  $ hg log
  changeset:   4:443431ffac4f
  tag:         tip
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
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  % after update 4, strip 4
  changeset:   3:65bd5f99a4a3
  tag:         tip
  parent:      1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     d
  
  $ teststrip 4 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % before update 4, strip 3
  changeset:   4:443431ffac4f
  tag:         tip
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
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  % after update 4, strip 2
  changeset:   3:443431ffac4f
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  
  $ teststrip 4 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % before update 4, strip 1
  changeset:   4:264128213d29
  tag:         tip
  parent:      1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  % after update 4, strip 1
  changeset:   0:9ab35a2d17cb
  tag:         tip
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
  tag:         tip
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
  tag:         tip
  parent:      1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  

  $ hg --traceback strip 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/264128213d29-0b39d6bf-backup.hg (glob)
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
  added 1 changesets with 0 changes to 0 files (+1 heads)
  new changesets 264128213d29
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ rm .hg/strip-backup/*
  $ hg log --graph
  o  changeset:   4:264128213d29
  |  tag:         tip
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
  tag:         tip
  parent:      1:ef3a871183d7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
  $ hg strip 4
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
  |  tag:         tip
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

  $ hg strip "roots(2)" 3
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  $ hg log -G
  @  changeset:   2:264128213d29
  |  tag:         tip
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
  |  tag:         tip
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

  $ hg strip 2 --config hooks.pretxnchangegroup.bad=false
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
  |  tag:         tip
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

  $ hg strip 2 4
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  $ hg log -G
  o  changeset:   2:65bd5f99a4a3
  |  tag:         tip
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

  $ hg strip 1 "2|4"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  $ restore

verify fncache is kept up-to-date

  $ touch a
  $ hg ci -qAm a
  $ cat .hg/store/fncache | sort
  data/a.i
  data/bar.i
  $ hg strip tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  $ cat .hg/store/fncache
  data/bar.i

stripping an empty revset

  $ hg strip "1 and not 1"
  abort: empty revision set
  [255]

remove branchy history for qimport tests

  $ hg strip 3
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)


strip of applied mq should cleanup status file

  $ echo "mq=" >> $HGRCPATH
  $ hg up -C 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo fooagain >> bar
  $ hg ci -mf
  $ hg qimport -r tip:2

applied patches before strip

  $ hg qapplied
  d
  e
  f

stripping revision in queue

  $ hg strip 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)

applied patches after stripping rev in queue

  $ hg qapplied
  d

stripping ancestor of queue

  $ hg strip 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)

applied patches after stripping ancestor of queue

  $ hg qapplied

Verify strip protects against stripping wc parent when there are uncommitted mods

  $ echo b > b
  $ echo bb > bar
  $ hg add b
  $ hg ci -m 'b'
  $ hg log --graph
  @  changeset:   1:76dcf9fab855
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:9ab35a2d17cb
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
  $ hg up 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > bar
  $ hg up -t false
  merging bar
  merging bar failed!
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]
  $ hg sum
  parent: 1:76dcf9fab855 tip
   b
  branch: default
  commit: 1 modified, 1 unknown, 1 unresolved
  update: (current)
  phases: 2 draft
  mq:     3 unapplied

  $ echo c > b
  $ hg strip tip
  abort: local changes found
  [255]
  $ hg strip tip --keep
  saved backup bundle to $TESTTMP/test/.hg/strip-backup/*-backup.hg (glob)
  $ hg log --graph
  @  changeset:   0:9ab35a2d17cb
     tag:         tip
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
  $ hg status
  M bar
  ? b
  ? bar.orig

  $ rm bar.orig
  $ hg sum
  parent: 0:9ab35a2d17cb tip
   a
  branch: default
  commit: 1 modified, 1 unknown
  update: (current)
  phases: 1 draft
  mq:     3 unapplied

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
  $ hg strip --keep tip
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
  $ hg strip --keep tip
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
  $ hg strip --keep '.^' -q
  $ cd ..

stripping many nodes on a complex graph (issue3299)

  $ hg init issue3299
  $ cd issue3299
  $ hg debugbuilddag '@a.:a@b.:b.:x<a@a.:a<b@b.:b<a@a.:a'
  $ hg strip 'not ancestors(x)'
  saved backup bundle to $TESTTMP/issue3299/.hg/strip-backup/*-backup.hg (glob)

test hg strip -B bookmark

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
  $ hg up -C todelete
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark todelete)
  $ hg strip -B nostrip
  bookmark 'nostrip' deleted
  abort: empty revision set
  [255]
  $ hg strip -B todelete
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/bookmarks/.hg/strip-backup/*-backup.hg (glob)
  bookmark 'todelete' deleted
  $ hg id -ir dcbb326fdec2
  abort: unknown revision 'dcbb326fdec2'!
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
  $ hg strip -B multipledelete1 -B multipledelete2
  saved backup bundle to $TESTTMP/bookmarks/.hg/strip-backup/e46a4836065c-89ec65c2-backup.hg (glob)
  bookmark 'multipledelete1' deleted
  bookmark 'multipledelete2' deleted
  $ hg id -ir e46a4836065c
  abort: unknown revision 'e46a4836065c'!
  [255]
  $ hg id -ir b4594d867745
  abort: unknown revision 'b4594d867745'!
  [255]
  $ hg strip -B singlenode1 -B singlenode2
  saved backup bundle to $TESTTMP/bookmarks/.hg/strip-backup/43227190fef8-8da858f2-backup.hg (glob)
  bookmark 'singlenode1' deleted
  bookmark 'singlenode2' deleted
  $ hg id -ir 43227190fef8
  abort: unknown revision '43227190fef8'!
  [255]
  $ hg strip -B unknownbookmark
  abort: bookmark 'unknownbookmark' not found
  [255]
  $ hg strip -B unknownbookmark1 -B unknownbookmark2
  abort: bookmark 'unknownbookmark1,unknownbookmark2' not found
  [255]
  $ hg strip -B delete -B unknownbookmark
  abort: bookmark 'unknownbookmark' not found
  [255]
  $ hg strip -B delete
  saved backup bundle to $TESTTMP/bookmarks/.hg/strip-backup/*-backup.hg (glob)
  bookmark 'delete' deleted
  $ hg id -ir 6:2702dd0c91e7
  abort: unknown revision '2702dd0c91e7'!
  [255]
  $ hg update B
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark B)
  $ echo a > a
  $ hg add a
  $ hg strip -B B
  abort: local changes found
  [255]
  $ hg bookmarks
   * B                         6:ff43616e5d0f

Make sure no one adds back a -b option:

  $ hg strip -b tip
  hg strip: option -b not recognized
  hg strip [-k] [-f] [-B bookmark] [-r] REV...
  
  strip changesets and all their descendants from the repository
  
  (use 'hg help -e strip' to show help for the strip extension)
  
  options ([+] can be repeated):
  
   -r --rev REV [+]        strip specified revision (optional, can specify
                           revisions without this option)
   -f --force              force removal of changesets, discard uncommitted
                           changes (no backup)
      --no-backup          no backups
   -k --keep               do not modify working directory during strip
   -B --bookmark VALUE [+] remove revs only reachable from given bookmark
      --mq                 operate on patch repository
  
  (use 'hg strip -h' to show more help)
  [255]

  $ cd ..

Verify bundles don't get overwritten:

  $ hg init doublebundle
  $ cd doublebundle
  $ touch a
  $ hg commit -Aqm a
  $ touch b
  $ hg commit -Aqm b
  $ hg strip -r 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/doublebundle/.hg/strip-backup/3903775176ed-e68910bd-backup.hg (glob)
  $ ls .hg/strip-backup
  3903775176ed-e68910bd-backup.hg
  $ hg pull -q -r 3903775176ed .hg/strip-backup/3903775176ed-e68910bd-backup.hg
  $ hg strip -r 0
  saved backup bundle to $TESTTMP/doublebundle/.hg/strip-backup/3903775176ed-54390173-backup.hg (glob)
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
  created new head
  $ hg up 'desc(commitC)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge 'desc(commitD)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'mergeCD'
  $ hg log -G
  @    changeset:   4:d8db9d137221
  |\   tag:         tip
  | |  parent:      2:5c51d8d6557d
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
  tag:         tip
  parent:      2:5c51d8d6557d
  parent:      3:6625a5168474
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     mergeCD
  

check strip behavior

  $ hg --config extensions.strip= strip 'desc(commitD)' --debug
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
  saved backup bundle to $TESTTMP/issue4736/.hg/strip-backup/6625a5168474-345bb43d-backup.hg (glob)
  updating the branch cache
  invalid branchheads cache (served): tip differs
  truncating cache/rbc-revs-v1 to 24
  $ hg log -G
  o  changeset:   2:5c51d8d6557d
  |  tag:         tip
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
  tag:         tip
  parent:      2:5c51d8d6557d
  parent:      3:6625a5168474
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     mergeCD
  
Check that the phase cache is properly invalidated after a strip with bookmark.

  $ cat > ../stripstalephasecache.py << EOF
  > from mercurial import extensions, localrepo
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
  $ hg strip ".^" --config extensions.crash=$TESTTMP/stripstalephasecache.py
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/issue4736/.hg/strip-backup/8f0b4384875c-4fa10deb-backup.hg (glob)
  $ hg up -C 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Error during post-close callback of the strip transaction
(They should be gracefully handled and reported)

  $ cat > ../crashstrip.py << EOF
  > from mercurial import error
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
  $ hg strip tip --config extensions.crash=$TESTTMP/crashstrip.py
  saved backup bundle to $TESTTMP/issue4736/.hg/strip-backup/5c51d8d6557d-70daef06-backup.hg (glob)
  strip failed, backup bundle stored in '$TESTTMP/issue4736/.hg/strip-backup/5c51d8d6557d-70daef06-backup.hg' (glob)
  abort: boom
  [255]

test stripping a working directory parent doesn't switch named branches

  $ hg log -G
  @  changeset:   1:eca11cf91c71
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     commitB
  |
  o  changeset:   0:105141ef12d0
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     commitA
  

  $ hg branch new-branch
  marked working directory as branch new-branch
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -m "start new branch"
  $ echo 'foo' > foo.txt
  $ hg ci -Aqm foo
  $ hg up default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'bar' > bar.txt
  $ hg ci -Aqm bar
  $ hg up new-branch
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg log -G
  @  changeset:   4:35358f982181
  |  tag:         tip
  |  parent:      1:eca11cf91c71
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     bar
  |
  | @  changeset:   3:f62c6c09b707
  | |  branch:      new-branch
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     foo
  | |
  | o  changeset:   2:b1d33a8cadd9
  |/   branch:      new-branch
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     start new branch
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
  

  $ hg strip --force -r 35358f982181
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/issue4736/.hg/strip-backup/35358f982181-50d992d4-backup.hg (glob)
  $ hg log -G
  @  changeset:   3:f62c6c09b707
  |  branch:      new-branch
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     foo
  |
  o  changeset:   2:b1d33a8cadd9
  |  branch:      new-branch
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     start new branch
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
  

  $ hg up default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'bar' > bar.txt
  $ hg ci -Aqm bar
  $ hg up new-branch
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m merge
  $ hg log -G
  @    changeset:   5:4cf5e92caec2
  |\   branch:      new-branch
  | |  tag:         tip
  | |  parent:      3:f62c6c09b707
  | |  parent:      4:35358f982181
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     merge
  | |
  | o  changeset:   4:35358f982181
  | |  parent:      1:eca11cf91c71
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     bar
  | |
  o |  changeset:   3:f62c6c09b707
  | |  branch:      new-branch
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     foo
  | |
  o |  changeset:   2:b1d33a8cadd9
  |/   branch:      new-branch
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     start new branch
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
  

  $ hg strip -r 35358f982181
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/issue4736/.hg/strip-backup/35358f982181-a6f020aa-backup.hg (glob)
  $ hg log -G
  @  changeset:   3:f62c6c09b707
  |  branch:      new-branch
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     foo
  |
  o  changeset:   2:b1d33a8cadd9
  |  branch:      new-branch
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     start new branch
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
  

  $ hg pull -u $TESTTMP/issue4736/.hg/strip-backup/35358f982181-a6f020aa-backup.hg
  pulling from $TESTTMP/issue4736/.hg/strip-backup/35358f982181-a6f020aa-backup.hg (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files
  new changesets 35358f982181:4cf5e92caec2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg strip -k -r 35358f982181
  saved backup bundle to $TESTTMP/issue4736/.hg/strip-backup/35358f982181-a6f020aa-backup.hg (glob)
  $ hg log -G
  @  changeset:   3:f62c6c09b707
  |  branch:      new-branch
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     foo
  |
  o  changeset:   2:b1d33a8cadd9
  |  branch:      new-branch
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     start new branch
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
  
  $ hg diff
  diff -r f62c6c09b707 bar.txt
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/bar.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +bar

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
  $ echo 3 >> I
  $ cat > $TESTTMP/delayedstrip.py <<EOF
  > from __future__ import absolute_import
  > from mercurial import commands, registrar, repair
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
  saved backup bundle to $TESTTMP/delayedstrip/.hg/strip-backup/f585351a92f8-17475721-I.hg (glob)

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
  > from mercurial import registrar, scmutil
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
  saved backup bundle to $TESTTMP/scmutilcleanup/.hg/strip-backup/f585351a92f8-73fb7c03-replace.hg (glob)

  $ hg log -G -T '{rev}:{node|short} {desc} {bookmarks}' -r 'sort(all(), topo)'
  o  8:1473d4b996d1 G2 b-F@divergent3 b-G
  |
  | o  7:d11b3456a873 F2 b-F
  | |
  | o  5:5cb05ba470a7 H
  |/|
  | o  3:7fb047a69f22 E b-F@divergent1
  | |
  | | o  6:7c78f703e465 D2 b-D
  | | |
  | | o  4:26805aba1e60 C
  | | |
  | | o  2:112478962961 B
  | |/
  o |  1:1fc8102cda62 G
   /
  o  0:426bada5c675 A b-B b-C b-I
  
  $ hg bookmark
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
  > stabilization.track-operation=1
  > EOF

  $ hg testnodescleanup --config extensions.t=$TESTTMP/scmutilcleanup.py

  $ rm .hg/localtags
  $ hg log -G -T '{rev}:{node|short} {desc} {bookmarks}' -r 'sort(all(), topo)'
  o  12:1473d4b996d1 G2 b-F@divergent3 b-G
  |
  | o  11:d11b3456a873 F2 b-F
  | |
  | o  8:5cb05ba470a7 H
  |/|
  | o  4:7fb047a69f22 E b-F@divergent1
  | |
  | | o  10:7c78f703e465 D2 b-D
  | | |
  | | x  6:26805aba1e60 C
  | | |
  | | x  3:112478962961 B
  | |/
  x |  1:1fc8102cda62 G
   /
  o  0:426bada5c675 A b-B b-C b-I
  
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
  $ hg strip .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/issue5678/.hg/strip-backup/489bac576828-bef27e14-backup.hg (glob)
  $ hg unbundle -q .hg/strip-backup/*
  $ hg debugobsolete
  cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b 489bac576828490c0bb8d45eac9e5e172e4ec0a8 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'test'}
  $ cd ..

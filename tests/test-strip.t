  $ echo "[format]" >> $HGRCPATH
  $ echo "usegeneraldelta=yes" >> $HGRCPATH
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "strip=" >> $HGRCPATH

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
  Stream params: {'Compression': 'BZ'}
  changegroup -- "{'version': '02'}"
      264128213d290d868c54642d13aeaa3675551a78
  $ hg pull .hg/strip-backup/*
  pulling from .hg/strip-backup/264128213d29-0b39d6bf-backup.hg
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
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
  

2 different branches: 2 strips

  $ hg strip 2 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
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
  
  (use "hg help -e strip" to show help for the strip extension)
  
  options ([+] can be repeated):
  
   -r --rev REV [+]        strip specified revision (optional, can specify
                           revisions without this option)
   -f --force              force removal of changesets, discard uncommitted
                           changes (no backup)
      --no-backup          no backups
   -k --keep               do not modify working directory during strip
   -B --bookmark VALUE [+] remove revs only reachable from given bookmark
      --mq                 operate on patch repository
  
  (use "hg strip -h" to show more help)
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
  bundle2-output-bundle: "HG20", (1 params) 1 parts total
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  saved backup bundle to $TESTTMP/issue4736/.hg/strip-backup/6625a5168474-345bb43d-backup.hg (glob)
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
  

Error during post-close callback of the strip transaction
(They should be gracefully handled and reported)

  $ cat > ../crashstrip.py << EOF
  > from mercurial import error
  > def reposetup(ui, repo):
  >     class crashstriprepo(repo.__class__):
  >         def transaction(self, desc, *args, **kwargs):
  >             tr = super(crashstriprepo, self).transaction(self, desc, *args, **kwargs)
  >             if desc == 'strip':
  >                 def crash(tra): raise error.Abort('boom')
  >                 tr.addpostclose('crash', crash)
  >             return tr
  >     repo.__class__ = crashstriprepo
  > EOF
  $ hg strip tip --config extensions.crash=$TESTTMP/crashstrip.py
  saved backup bundle to $TESTTMP/issue4736/.hg/strip-backup/5c51d8d6557d-70daef06-backup.hg (glob)
  strip failed, full bundle stored in '$TESTTMP/issue4736/.hg/strip-backup/5c51d8d6557d-70daef06-backup.hg' (glob)
  abort: boom
  [255]



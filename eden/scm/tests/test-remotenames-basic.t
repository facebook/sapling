  > echo "[extensions]" >> $HGRCPATH
  > echo "remotenames=" >> $HGRCPATH

  > FILTERPWD="sed s%`pwd`/%%g"

  > mkcommit()
  > {
  >    echo $1 > $1
  >    hg add $1
  >    hg ci -m "add $1"
  > }

  $ disable treemanifest

Test that remotenames works on a repo without any names file

  $ hg init alpha
  $ cd alpha
  $ mkcommit a
  $ mkcommit b
  $ hg log -r 'upstream()'
  $ hg log -r . -T '{remotenames} {remotebookmarks}\n'
   

Continue testing

  $ mkcommit c
  $ cd ..
  $ hg clone alpha beta | $FILTERPWD
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd beta
  $ hg book babar
  $ mkcommit d
  $ cd ..

  $ hg init gamma
  $ cd gamma
  $ cat > .hg/hgrc <<EOF
  > [paths]
  > default = ../alpha
  > alpha = ../alpha
  > beta = ../beta
  > EOF
  $ hg pull | $FILTERPWD
  pulling from alpha
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  $ hg pull beta | $FILTERPWD
  pulling from beta
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ hg co -C default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit e

graph shows tags for the branch heads of each path
  $ hg log --graph
  @  changeset:   4:9d206ffc875e
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add e
  |
  o  changeset:   3:47d2a3944de8
  |  bookmark:    beta/babar
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add d
  |
  o  changeset:   2:4538525df7e2
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add c
  |
  o  changeset:   1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add b
  |
  o  changeset:   0:1f0dee641bb7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  

make sure we can list remote bookmarks with --all

  $ hg bookmarks --all
  no bookmarks set
     beta/babar                3:47d2a3944de8

  $ hg bookmarks --all -T json
  [
   {
    "node": "47d2a3944de8b013de3be9578e8e344ea2e6c097",
    "remotebookmark": "beta/babar",
    "rev": 3
   }
  ]
  $ hg bookmarks --remote
     beta/babar                3:47d2a3944de8

Verify missing node doesnt break remotenames

  $ echo "18f8e0f8ba54270bf158734c781327581cf43634 bookmarks beta/foo" >> .hg/store/remotenames
  $ hg book --remote --config remotenames.resolvenodes=False
     beta/babar                3:47d2a3944de8

make sure bogus revisions in .hg/store/remotenames do not break hg
  $ echo deadbeefdeadbeefdeadbeefdeadbeefdeadbeef default/default >> \
  > .hg/store/remotenames
  $ hg parents
  changeset:   4:9d206ffc875e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add e
  
Verify that the revsets operate as expected:
  $ hg log -r 'not pushed()'
  changeset:   4:9d206ffc875e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add e
  


Upstream without configuration is synonymous with upstream('default'):
  $ hg log -r 'not upstream()'
  changeset:   0:1f0dee641bb7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  
  changeset:   1:7c3bad9141dc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add b
  
  changeset:   2:4538525df7e2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add c
  
  changeset:   3:47d2a3944de8
  bookmark:    beta/babar
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add d
  
  changeset:   4:9d206ffc875e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add e
  

but configured, it'll do the expected thing:
  $ echo '[remotenames]' >> .hg/hgrc
  $ echo 'upstream=alpha' >> .hg/hgrc
  $ hg log --graph -r 'not upstream()'
  @  changeset:   4:9d206ffc875e
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add e
  |
  o  changeset:   3:47d2a3944de8
  |  bookmark:    beta/babar
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add d
  |
  o  changeset:   2:4538525df7e2
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add c
  |
  o  changeset:   1:7c3bad9141dc
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add b
  |
  o  changeset:   0:1f0dee641bb7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  
  $ hg log --limit 2 --graph -r 'heads(upstream())'

Test remotenames revset and keyword

  $ hg log -r 'remotenames()' \
  >   --template '{rev}:{node|short} {remotenames}\n'
  3:47d2a3944de8 beta/babar

Test remotebookmark revsets

  $ hg log -r 'remotebookmark()' \
  >   --template '{rev}:{node|short} {remotebookmarks}\n'
  3:47d2a3944de8 beta/babar
  $ hg log -r 'remotebookmark("beta/babar")' \
  >   --template '{rev}:{node|short} {remotebookmarks}\n'
  3:47d2a3944de8 beta/babar
  $ hg log -r 'remotebookmark("beta/stable")' \
  >   --template '{rev}:{node|short} {remotebookmarks}\n'
  abort: no remote bookmarks exist that match 'beta/stable'!
  [255]
  $ hg log -r 'remotebookmark("re:beta/.*")' \
  >   --template '{rev}:{node|short} {remotebookmarks}\n'
  3:47d2a3944de8 beta/babar
  $ hg log -r 'remotebookmark("re:gamma/.*")' \
  >   --template '{rev}:{node|short} {remotebookmarks}\n'
  abort: no remote bookmarks exist that match 're:gamma/.*'!
  [255]

Test clone --mirror

  $ cd ..
  $ cd alpha
  $ hg book foo bar baz
  $ cd ..
  $ hg clone --mirror alpha mirror
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd mirror
  $ hg book
     bar                       2:4538525df7e2
     baz                       2:4538525df7e2
     foo                       2:4538525df7e2

Test loading with hggit

  $ . "$TESTDIR/hggit/testutil"
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "hggit=" >> $HGRCPATH
  $ echo "[devel]" >> $HGRCPATH
  $ echo "all-warnings=no" >> $HGRCPATH
  $ hg help bookmarks  | egrep -- '--(un){0,1}track'
   -t --track BOOKMARK     track this bookmark or remote name
   -u --untrack            remove tracking for this bookmark

Test json formatted bookmarks with tracking data
  $ cd ..
  $ hg init delta
  $ cd delta
  $ hg book mimimi -t lalala
  $ hg book -v -T json
  [
   {
    "active": true,
    "bookmark": "mimimi",
    "node": "0000000000000000000000000000000000000000",
    "rev": -1,
    "tracking": "lalala"
   }
  ]
  $ hg book -v
   * mimimi                    -1:000000000000           [lalala]

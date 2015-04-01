#require json
#require serve

  $ request() {
  >   $TESTDIR/get-with-headers.py --json localhost:$HGPORT "$1"
  > }

  $ hg init test
  $ cd test
  $ mkdir da
  $ echo foo > da/foo
  $ echo foo > foo
  $ hg -q ci -A -m initial
  $ echo bar > foo
  $ hg ci -m 'modify foo'
  $ echo bar > da/foo
  $ hg ci -m 'modify da/foo'
  $ hg bookmark bookmark1
  $ hg up default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark bookmark1)
  $ hg mv foo foo-new
  $ hg commit -m 'move foo'
  $ hg tag -m 'create tag' tag1
  $ echo baz > da/foo
  $ hg commit -m 'another commit to da/foo'
  $ hg tag -m 'create tag2' tag2
  $ hg bookmark bookmark2
  $ hg -q up -r 0
  $ hg -q branch test-branch
  $ echo branch > foo
  $ hg commit -m 'create test branch'

  $ hg log -G
  @  changeset:   7:6ab967a8ab34
  |  branch:      test-branch
  |  tag:         tip
  |  parent:      0:06e557f3edf6
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     create test branch
  |
  | o  changeset:   6:ceed296fe500
  | |  bookmark:    bookmark2
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     create tag2
  | |
  | o  changeset:   5:f2890a05fea4
  | |  tag:         tag2
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     another commit to da/foo
  | |
  | o  changeset:   4:93a8ce14f891
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     create tag
  | |
  | o  changeset:   3:78896eb0e102
  | |  tag:         tag1
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     move foo
  | |
  | o  changeset:   2:8d7c456572ac
  | |  bookmark:    bookmark1
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     modify da/foo
  | |
  | o  changeset:   1:f8bbb9024b10
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     modify foo
  |
  o  changeset:   0:06e557f3edf6
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     initial
  

  $ hg serve -p $HGPORT -d --pid-file=hg.pid -A access.log -E error.log
  $ cat hg.pid >> $DAEMON_PIDS

(Try to keep these in roughly the order they are defined in webcommands.py)

(log is handled by filelog/ and changelog/ - ignore it)

(rawfile/ doesn't use templating - nothing to test)

file/{revision}/{path} shows file revision

  $ request json-file/06e557f3edf6/foo
  200 Script output follows
  
  "not yet implemented"

file/{revision} shows root directory info

  $ request json-file/06e557f3edf6
  200 Script output follows
  
  "not yet implemented"

changelog/ shows information about several changesets

  $ request json-changelog
  200 Script output follows
  
  "not yet implemented"

changelog/{revision} shows information about a single changeset

  $ request json-changelog/06e557f3edf6
  200 Script output follows
  
  "not yet implemented"

shortlog/ shows information about a set of changesets

  $ request json-shortlog
  200 Script output follows
  
  "not yet implemented"

changeset/ renders the tip changeset

  $ request json-rev
  200 Script output follows
  
  "not yet implemented"

changeset/{revision} shows tags

  $ request json-rev/78896eb0e102
  200 Script output follows
  
  "not yet implemented"

changeset/{revision} shows bookmarks

  $ request json-rev/8d7c456572ac
  200 Script output follows
  
  "not yet implemented"

changeset/{revision} shows branches

  $ request json-rev/6ab967a8ab34
  200 Script output follows
  
  "not yet implemented"

manifest/{revision}/{path} shows info about a directory at a revision

  $ request json-manifest/06e557f3edf6/
  200 Script output follows
  
  "not yet implemented"

tags/ shows tags info

  $ request json-tags
  200 Script output follows
  
  {
    "node": "6ab967a8ab3489227a83f80e920faa039a71819f",
    "tags": [
      {
        "date": [
          0.0,
          0
        ],
        "node": "f2890a05fea49bfaf9fb27ed5490894eba32da78",
        "tag": "tag2"
      },
      {
        "date": [
          0.0,
          0
        ],
        "node": "78896eb0e102174ce9278438a95e12543e4367a7",
        "tag": "tag1"
      }
    ]
  }

bookmarks/ shows bookmarks info

  $ request json-bookmarks
  200 Script output follows
  
  {
    "bookmarks": [
      {
        "bookmark": "bookmark1",
        "date": [
          0.0,
          0
        ],
        "node": "8d7c456572acf3557e8ed8a07286b10c408bcec5"
      },
      {
        "bookmark": "bookmark2",
        "date": [
          0.0,
          0
        ],
        "node": "ceed296fe500c3fac9541e31dad860cb49c89e45"
      }
    ],
    "node": "6ab967a8ab3489227a83f80e920faa039a71819f"
  }

branches/ shows branches info

  $ request json-branches
  200 Script output follows
  
  {
    "branches": [
      {
        "branch": "test-branch",
        "date": [
          0.0,
          0
        ],
        "node": "6ab967a8ab3489227a83f80e920faa039a71819f",
        "status": "open"
      },
      {
        "branch": "default",
        "date": [
          0.0,
          0
        ],
        "node": "ceed296fe500c3fac9541e31dad860cb49c89e45",
        "status": "open"
      }
    ]
  }

summary/ shows a summary of repository state

  $ request json-summary
  200 Script output follows
  
  "not yet implemented"

filediff/{revision}/{path} shows changes to a file in a revision

  $ request json-diff/f8bbb9024b10/foo
  200 Script output follows
  
  "not yet implemented"

comparison/{revision}/{path} shows information about before and after for a file

  $ request json-comparison/f8bbb9024b10/foo
  200 Script output follows
  
  "not yet implemented"

annotate/{revision}/{path} shows annotations for each line

  $ request json-annotate/f8bbb9024b10/foo
  200 Script output follows
  
  "not yet implemented"

filelog/{revision}/{path} shows history of a single file

  $ request json-filelog/f8bbb9024b10/foo
  200 Script output follows
  
  "not yet implemented"

(archive/ doesn't use templating, so ignore it)

(static/ doesn't use templating, so ignore it)

graph/ shows information that can be used to render a graph of the DAG

  $ request json-graph
  200 Script output follows
  
  "not yet implemented"

help/ shows help topics

  $ request json-help
  200 Script output follows
  
  "not yet implemented"

help/{topic} shows an individual help topic

  $ request json-help/phases
  200 Script output follows
  
  "not yet implemented"

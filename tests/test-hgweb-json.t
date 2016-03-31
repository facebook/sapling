#require serve

  $ request() {
  >   get-with-headers.py --json localhost:$HGPORT "$1"
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
  $ hg phase --public -r .
  $ echo baz > da/foo
  $ hg commit -m 'another commit to da/foo'
  $ hg tag -m 'create tag2' tag2
  $ hg bookmark bookmark2
  $ hg -q up -r 0
  $ hg -q branch test-branch
  $ echo branch > foo
  $ hg commit -m 'create test branch'
  $ echo branch_commit_2 > foo
  $ hg commit -m 'another commit in test-branch'
  $ hg -q up default
  $ hg merge --tool :local test-branch
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m 'merge test-branch into default'

  $ hg log -G
  @    changeset:   9:cc725e08502a
  |\   tag:         tip
  | |  parent:      6:ceed296fe500
  | |  parent:      8:ed66c30e87eb
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     merge test-branch into default
  | |
  | o  changeset:   8:ed66c30e87eb
  | |  branch:      test-branch
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     another commit in test-branch
  | |
  | o  changeset:   7:6ab967a8ab34
  | |  branch:      test-branch
  | |  parent:      0:06e557f3edf6
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     create test branch
  | |
  o |  changeset:   6:ceed296fe500
  | |  bookmark:    bookmark2
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     create tag2
  | |
  o |  changeset:   5:f2890a05fea4
  | |  tag:         tag2
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     another commit to da/foo
  | |
  o |  changeset:   4:93a8ce14f891
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     create tag
  | |
  o |  changeset:   3:78896eb0e102
  | |  tag:         tag1
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     move foo
  | |
  o |  changeset:   2:8d7c456572ac
  | |  bookmark:    bookmark1
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     modify da/foo
  | |
  o |  changeset:   1:f8bbb9024b10
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

  $ request json-file/cc725e08502a
  200 Script output follows
  
  {
    "abspath": "/",
    "bookmarks": [],
    "directories": [
      {
        "abspath": "/da",
        "basename": "da",
        "emptydirs": ""
      }
    ],
    "files": [
      {
        "abspath": ".hgtags",
        "basename": ".hgtags",
        "date": [
          0.0,
          0
        ],
        "flags": "",
        "size": 92
      },
      {
        "abspath": "foo-new",
        "basename": "foo-new",
        "date": [
          0.0,
          0
        ],
        "flags": "",
        "size": 4
      }
    ],
    "node": "cc725e08502a79dd1eda913760fbe06ed7a9abc7",
    "tags": [
      "tip"
    ]
  }

changelog/ shows information about several changesets

  $ request json-changelog
  200 Script output follows
  
  {
    "changeset_count": 10,
    "changesets": [
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "merge test-branch into default",
        "node": "cc725e08502a79dd1eda913760fbe06ed7a9abc7",
        "parents": [
          "ceed296fe500c3fac9541e31dad860cb49c89e45",
          "ed66c30e87eb65337c05a4229efaa5f1d5285a90"
        ],
        "tags": [
          "tip"
        ],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "another commit in test-branch",
        "node": "ed66c30e87eb65337c05a4229efaa5f1d5285a90",
        "parents": [
          "6ab967a8ab3489227a83f80e920faa039a71819f"
        ],
        "tags": [],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "create test branch",
        "node": "6ab967a8ab3489227a83f80e920faa039a71819f",
        "parents": [
          "06e557f3edf66faa1ccaba5dd8c203c21cc79f1e"
        ],
        "tags": [],
        "user": "test"
      },
      {
        "bookmarks": [
          "bookmark2"
        ],
        "date": [
          0.0,
          0
        ],
        "desc": "create tag2",
        "node": "ceed296fe500c3fac9541e31dad860cb49c89e45",
        "parents": [
          "f2890a05fea49bfaf9fb27ed5490894eba32da78"
        ],
        "tags": [],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "another commit to da/foo",
        "node": "f2890a05fea49bfaf9fb27ed5490894eba32da78",
        "parents": [
          "93a8ce14f89156426b7fa981af8042da53f03aa0"
        ],
        "tags": [
          "tag2"
        ],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "create tag",
        "node": "93a8ce14f89156426b7fa981af8042da53f03aa0",
        "parents": [
          "78896eb0e102174ce9278438a95e12543e4367a7"
        ],
        "tags": [],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "move foo",
        "node": "78896eb0e102174ce9278438a95e12543e4367a7",
        "parents": [
          "8d7c456572acf3557e8ed8a07286b10c408bcec5"
        ],
        "tags": [
          "tag1"
        ],
        "user": "test"
      },
      {
        "bookmarks": [
          "bookmark1"
        ],
        "date": [
          0.0,
          0
        ],
        "desc": "modify da/foo",
        "node": "8d7c456572acf3557e8ed8a07286b10c408bcec5",
        "parents": [
          "f8bbb9024b10f93cdbb8d940337398291d40dea8"
        ],
        "tags": [],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "modify foo",
        "node": "f8bbb9024b10f93cdbb8d940337398291d40dea8",
        "parents": [
          "06e557f3edf66faa1ccaba5dd8c203c21cc79f1e"
        ],
        "tags": [],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "initial",
        "node": "06e557f3edf66faa1ccaba5dd8c203c21cc79f1e",
        "parents": [],
        "tags": [],
        "user": "test"
      }
    ],
    "node": "cc725e08502a79dd1eda913760fbe06ed7a9abc7"
  }

changelog/{revision} shows information starting at a specific changeset

  $ request json-changelog/f8bbb9024b10
  200 Script output follows
  
  {
    "changeset_count": 10,
    "changesets": [
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "modify foo",
        "node": "f8bbb9024b10f93cdbb8d940337398291d40dea8",
        "parents": [
          "06e557f3edf66faa1ccaba5dd8c203c21cc79f1e"
        ],
        "tags": [],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "initial",
        "node": "06e557f3edf66faa1ccaba5dd8c203c21cc79f1e",
        "parents": [],
        "tags": [],
        "user": "test"
      }
    ],
    "node": "f8bbb9024b10f93cdbb8d940337398291d40dea8"
  }

shortlog/ shows information about a set of changesets

  $ request json-shortlog
  200 Script output follows
  
  {
    "changeset_count": 10,
    "changesets": [
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "merge test-branch into default",
        "node": "cc725e08502a79dd1eda913760fbe06ed7a9abc7",
        "parents": [
          "ceed296fe500c3fac9541e31dad860cb49c89e45",
          "ed66c30e87eb65337c05a4229efaa5f1d5285a90"
        ],
        "tags": [
          "tip"
        ],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "another commit in test-branch",
        "node": "ed66c30e87eb65337c05a4229efaa5f1d5285a90",
        "parents": [
          "6ab967a8ab3489227a83f80e920faa039a71819f"
        ],
        "tags": [],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "create test branch",
        "node": "6ab967a8ab3489227a83f80e920faa039a71819f",
        "parents": [
          "06e557f3edf66faa1ccaba5dd8c203c21cc79f1e"
        ],
        "tags": [],
        "user": "test"
      },
      {
        "bookmarks": [
          "bookmark2"
        ],
        "date": [
          0.0,
          0
        ],
        "desc": "create tag2",
        "node": "ceed296fe500c3fac9541e31dad860cb49c89e45",
        "parents": [
          "f2890a05fea49bfaf9fb27ed5490894eba32da78"
        ],
        "tags": [],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "another commit to da/foo",
        "node": "f2890a05fea49bfaf9fb27ed5490894eba32da78",
        "parents": [
          "93a8ce14f89156426b7fa981af8042da53f03aa0"
        ],
        "tags": [
          "tag2"
        ],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "create tag",
        "node": "93a8ce14f89156426b7fa981af8042da53f03aa0",
        "parents": [
          "78896eb0e102174ce9278438a95e12543e4367a7"
        ],
        "tags": [],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "move foo",
        "node": "78896eb0e102174ce9278438a95e12543e4367a7",
        "parents": [
          "8d7c456572acf3557e8ed8a07286b10c408bcec5"
        ],
        "tags": [
          "tag1"
        ],
        "user": "test"
      },
      {
        "bookmarks": [
          "bookmark1"
        ],
        "date": [
          0.0,
          0
        ],
        "desc": "modify da/foo",
        "node": "8d7c456572acf3557e8ed8a07286b10c408bcec5",
        "parents": [
          "f8bbb9024b10f93cdbb8d940337398291d40dea8"
        ],
        "tags": [],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "modify foo",
        "node": "f8bbb9024b10f93cdbb8d940337398291d40dea8",
        "parents": [
          "06e557f3edf66faa1ccaba5dd8c203c21cc79f1e"
        ],
        "tags": [],
        "user": "test"
      },
      {
        "bookmarks": [],
        "date": [
          0.0,
          0
        ],
        "desc": "initial",
        "node": "06e557f3edf66faa1ccaba5dd8c203c21cc79f1e",
        "parents": [],
        "tags": [],
        "user": "test"
      }
    ],
    "node": "cc725e08502a79dd1eda913760fbe06ed7a9abc7"
  }

changeset/ renders the tip changeset

  $ request json-rev
  200 Script output follows
  
  {
    "bookmarks": [],
    "branch": "default",
    "date": [
      0.0,
      0
    ],
    "desc": "merge test-branch into default",
    "node": "cc725e08502a79dd1eda913760fbe06ed7a9abc7",
    "parents": [
      "ceed296fe500c3fac9541e31dad860cb49c89e45",
      "ed66c30e87eb65337c05a4229efaa5f1d5285a90"
    ],
    "phase": "draft",
    "tags": [
      "tip"
    ],
    "user": "test"
  }

changeset/{revision} shows tags

  $ request json-rev/78896eb0e102
  200 Script output follows
  
  {
    "bookmarks": [],
    "branch": "default",
    "date": [
      0.0,
      0
    ],
    "desc": "move foo",
    "node": "78896eb0e102174ce9278438a95e12543e4367a7",
    "parents": [
      "8d7c456572acf3557e8ed8a07286b10c408bcec5"
    ],
    "phase": "public",
    "tags": [
      "tag1"
    ],
    "user": "test"
  }

changeset/{revision} shows bookmarks

  $ request json-rev/8d7c456572ac
  200 Script output follows
  
  {
    "bookmarks": [
      "bookmark1"
    ],
    "branch": "default",
    "date": [
      0.0,
      0
    ],
    "desc": "modify da/foo",
    "node": "8d7c456572acf3557e8ed8a07286b10c408bcec5",
    "parents": [
      "f8bbb9024b10f93cdbb8d940337398291d40dea8"
    ],
    "phase": "public",
    "tags": [],
    "user": "test"
  }

changeset/{revision} shows branches

  $ request json-rev/6ab967a8ab34
  200 Script output follows
  
  {
    "bookmarks": [],
    "branch": "test-branch",
    "date": [
      0.0,
      0
    ],
    "desc": "create test branch",
    "node": "6ab967a8ab3489227a83f80e920faa039a71819f",
    "parents": [
      "06e557f3edf66faa1ccaba5dd8c203c21cc79f1e"
    ],
    "phase": "draft",
    "tags": [],
    "user": "test"
  }

manifest/{revision}/{path} shows info about a directory at a revision

  $ request json-manifest/06e557f3edf6/
  200 Script output follows
  
  {
    "abspath": "/",
    "bookmarks": [],
    "directories": [
      {
        "abspath": "/da",
        "basename": "da",
        "emptydirs": ""
      }
    ],
    "files": [
      {
        "abspath": "foo",
        "basename": "foo",
        "date": [
          0.0,
          0
        ],
        "flags": "",
        "size": 4
      }
    ],
    "node": "06e557f3edf66faa1ccaba5dd8c203c21cc79f1e",
    "tags": []
  }

tags/ shows tags info

  $ request json-tags
  200 Script output follows
  
  {
    "node": "cc725e08502a79dd1eda913760fbe06ed7a9abc7",
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
        "bookmark": "bookmark2",
        "date": [
          0.0,
          0
        ],
        "node": "ceed296fe500c3fac9541e31dad860cb49c89e45"
      },
      {
        "bookmark": "bookmark1",
        "date": [
          0.0,
          0
        ],
        "node": "8d7c456572acf3557e8ed8a07286b10c408bcec5"
      }
    ],
    "node": "cc725e08502a79dd1eda913760fbe06ed7a9abc7"
  }

branches/ shows branches info

  $ request json-branches
  200 Script output follows
  
  {
    "branches": [
      {
        "branch": "default",
        "date": [
          0.0,
          0
        ],
        "node": "cc725e08502a79dd1eda913760fbe06ed7a9abc7",
        "status": "open"
      },
      {
        "branch": "test-branch",
        "date": [
          0.0,
          0
        ],
        "node": "ed66c30e87eb65337c05a4229efaa5f1d5285a90",
        "status": "inactive"
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
  
  {
    "author": "test",
    "children": [],
    "date": [
      0.0,
      0
    ],
    "desc": "modify foo",
    "diff": [
      {
        "blockno": 1,
        "lines": [
          {
            "l": "--- a/foo\tThu Jan 01 00:00:00 1970 +0000\n",
            "n": 1,
            "t": "-"
          },
          {
            "l": "+++ b/foo\tThu Jan 01 00:00:00 1970 +0000\n",
            "n": 2,
            "t": "+"
          },
          {
            "l": "@@ -1,1 +1,1 @@\n",
            "n": 3,
            "t": "@"
          },
          {
            "l": "-foo\n",
            "n": 4,
            "t": "-"
          },
          {
            "l": "+bar\n",
            "n": 5,
            "t": "+"
          }
        ]
      }
    ],
    "node": "f8bbb9024b10f93cdbb8d940337398291d40dea8",
    "parents": [
      "06e557f3edf66faa1ccaba5dd8c203c21cc79f1e"
    ],
    "path": "foo"
  }

comparison/{revision}/{path} shows information about before and after for a file

  $ request json-comparison/f8bbb9024b10/foo
  200 Script output follows
  
  {
    "author": "test",
    "children": [],
    "comparison": [
      {
        "lines": [
          {
            "ll": "foo",
            "ln": 1,
            "rl": "bar",
            "rn": 1,
            "t": "replace"
          }
        ]
      }
    ],
    "date": [
      0.0,
      0
    ],
    "desc": "modify foo",
    "leftnode": "06e557f3edf66faa1ccaba5dd8c203c21cc79f1e",
    "node": "f8bbb9024b10f93cdbb8d940337398291d40dea8",
    "parents": [
      "06e557f3edf66faa1ccaba5dd8c203c21cc79f1e"
    ],
    "path": "foo",
    "rightnode": "f8bbb9024b10f93cdbb8d940337398291d40dea8"
  }

annotate/{revision}/{path} shows annotations for each line

  $ request json-annotate/f8bbb9024b10/foo
  200 Script output follows
  
  {
    "abspath": "foo",
    "annotate": [
      {
        "abspath": "foo",
        "author": "test",
        "desc": "modify foo",
        "line": "bar\n",
        "lineno": 1,
        "node": "f8bbb9024b10f93cdbb8d940337398291d40dea8",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 1
      }
    ],
    "author": "test",
    "children": [],
    "date": [
      0.0,
      0
    ],
    "desc": "modify foo",
    "node": "f8bbb9024b10f93cdbb8d940337398291d40dea8",
    "parents": [
      "06e557f3edf66faa1ccaba5dd8c203c21cc79f1e"
    ],
    "permissions": ""
  }

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
  
  {
    "earlycommands": [
      {
        "summary": "add the specified files on the next commit",
        "topic": "add"
      },
      {
        "summary": "show changeset information by line for each file",
        "topic": "annotate"
      },
      {
        "summary": "make a copy of an existing repository",
        "topic": "clone"
      },
      {
        "summary": "commit the specified files or all outstanding changes",
        "topic": "commit"
      },
      {
        "summary": "diff repository (or selected files)",
        "topic": "diff"
      },
      {
        "summary": "dump the header and diffs for one or more changesets",
        "topic": "export"
      },
      {
        "summary": "forget the specified files on the next commit",
        "topic": "forget"
      },
      {
        "summary": "create a new repository in the given directory",
        "topic": "init"
      },
      {
        "summary": "show revision history of entire repository or files",
        "topic": "log"
      },
      {
        "summary": "merge another revision into working directory",
        "topic": "merge"
      },
      {
        "summary": "pull changes from the specified source",
        "topic": "pull"
      },
      {
        "summary": "push changes to the specified destination",
        "topic": "push"
      },
      {
        "summary": "remove the specified files on the next commit",
        "topic": "remove"
      },
      {
        "summary": "start stand-alone webserver",
        "topic": "serve"
      },
      {
        "summary": "show changed files in the working directory",
        "topic": "status"
      },
      {
        "summary": "summarize working directory state",
        "topic": "summary"
      },
      {
        "summary": "update working directory (or switch revisions)",
        "topic": "update"
      }
    ],
    "othercommands": [
      {
        "summary": "add all new files, delete all missing files",
        "topic": "addremove"
      },
      {
        "summary": "create an unversioned archive of a repository revision",
        "topic": "archive"
      },
      {
        "summary": "reverse effect of earlier changeset",
        "topic": "backout"
      },
      {
        "summary": "subdivision search of changesets",
        "topic": "bisect"
      },
      {
        "summary": "create a new bookmark or list existing bookmarks",
        "topic": "bookmarks"
      },
      {
        "summary": "set or show the current branch name",
        "topic": "branch"
      },
      {
        "summary": "list repository named branches",
        "topic": "branches"
      },
      {
        "summary": "create a changegroup file",
        "topic": "bundle"
      },
      {
        "summary": "output the current or given revision of files",
        "topic": "cat"
      },
      {
        "summary": "show combined config settings from all hgrc files",
        "topic": "config"
      },
      {
        "summary": "mark files as copied for the next commit",
        "topic": "copy"
      },
      {
        "summary": "list tracked files",
        "topic": "files"
      },
      {
        "summary": "copy changes from other branches onto the current branch",
        "topic": "graft"
      },
      {
        "summary": "search for a pattern in specified files and revisions",
        "topic": "grep"
      },
      {
        "summary": "show branch heads",
        "topic": "heads"
      },
      {
        "summary": "show help for a given topic or a help overview",
        "topic": "help"
      },
      {
        "summary": "identify the working directory or specified revision",
        "topic": "identify"
      },
      {
        "summary": "import an ordered set of patches",
        "topic": "import"
      },
      {
        "summary": "show new changesets found in source",
        "topic": "incoming"
      },
      {
        "summary": "output the current or given revision of the project manifest",
        "topic": "manifest"
      },
      {
        "summary": "show changesets not found in the destination",
        "topic": "outgoing"
      },
      {
        "summary": "show aliases for remote repositories",
        "topic": "paths"
      },
      {
        "summary": "set or show the current phase name",
        "topic": "phase"
      },
      {
        "summary": "roll back an interrupted transaction",
        "topic": "recover"
      },
      {
        "summary": "rename files; equivalent of copy + remove",
        "topic": "rename"
      },
      {
        "summary": "redo merges or set/view the merge status of files",
        "topic": "resolve"
      },
      {
        "summary": "restore files to their checkout state",
        "topic": "revert"
      },
      {
        "summary": "print the root (top) of the current working directory",
        "topic": "root"
      },
      {
        "summary": "add one or more tags for the current or given revision",
        "topic": "tag"
      },
      {
        "summary": "list repository tags",
        "topic": "tags"
      },
      {
        "summary": "apply one or more changegroup files",
        "topic": "unbundle"
      },
      {
        "summary": "verify the integrity of the repository",
        "topic": "verify"
      },
      {
        "summary": "output version and copyright information",
        "topic": "version"
      }
    ],
    "topics": [
      {
        "summary": "Configuration Files",
        "topic": "config"
      },
      {
        "summary": "Date Formats",
        "topic": "dates"
      },
      {
        "summary": "Diff Formats",
        "topic": "diffs"
      },
      {
        "summary": "Environment Variables",
        "topic": "environment"
      },
      {
        "summary": "Using Additional Features",
        "topic": "extensions"
      },
      {
        "summary": "Specifying File Sets",
        "topic": "filesets"
      },
      {
        "summary": "Glossary",
        "topic": "glossary"
      },
      {
        "summary": "Syntax for Mercurial Ignore Files",
        "topic": "hgignore"
      },
      {
        "summary": "Configuring hgweb",
        "topic": "hgweb"
      },
      {
        "summary": "Technical implementation topics",
        "topic": "internals"
      },
      {
        "summary": "Merge Tools",
        "topic": "merge-tools"
      },
      {
        "summary": "Specifying Multiple Revisions",
        "topic": "multirevs"
      },
      {
        "summary": "File Name Patterns",
        "topic": "patterns"
      },
      {
        "summary": "Working with Phases",
        "topic": "phases"
      },
      {
        "summary": "Specifying Single Revisions",
        "topic": "revisions"
      },
      {
        "summary": "Specifying Revision Sets",
        "topic": "revsets"
      },
      {
        "summary": "Using Mercurial from scripts and automation",
        "topic": "scripting"
      },
      {
        "summary": "Subrepositories",
        "topic": "subrepos"
      },
      {
        "summary": "Template Usage",
        "topic": "templating"
      },
      {
        "summary": "URL Paths",
        "topic": "urls"
      }
    ]
  }

help/{topic} shows an individual help topic

  $ request json-help/phases
  200 Script output follows
  
  {
    "rawdoc": "Working with Phases\n*", (glob)
    "topic": "phases"
  }

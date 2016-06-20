  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/reflog.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > reflog=$TESTTMP/reflog.py
  > EOF

We want to import extutil from the repo and not the system one
  $ PYTHONPATH=$extpath:$PYTHONPATH
  $ export PYTHONPATH

  $ hg init repo
  $ cd repo

Test empty reflog

  $ hg reflog
  Previous locations of '.':
  no recorded locations
  $ hg reflog fakebookmark
  Previous locations of 'fakebookmark':
  no recorded locations

Test that working copy changes are tracked

  $ echo a > a
  $ hg commit -Aqm a
  $ hg reflog
  Previous locations of '.':
  cb9a9f314b8b  commit -Aqm a
  $ echo b > a
  $ hg commit -Aqm b
  $ hg reflog
  Previous locations of '.':
  1e6c11564562  commit -Aqm b
  cb9a9f314b8b  commit -Aqm a
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg reflog
  Previous locations of '.':
  cb9a9f314b8b  up 0
  1e6c11564562  commit -Aqm b
  cb9a9f314b8b  commit -Aqm a

Test that bookmarks are tracked

  $ hg book -r tip foo
  $ hg reflog foo
  Previous locations of 'foo':
  1e6c11564562  book -r tip foo
  $ hg book  -f foo
  $ hg reflog foo
  Previous locations of 'foo':
  cb9a9f314b8b  book -f foo
  1e6c11564562  book -r tip foo
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updating bookmark foo
  $ hg reflog foo
  Previous locations of 'foo':
  1e6c11564562  up
  cb9a9f314b8b  book -f foo
  1e6c11564562  book -r tip foo

Test that bookmarks and working copy tracking is not mixed

  $ hg reflog
  Previous locations of '.':
  1e6c11564562  up
  cb9a9f314b8b  up 0
  1e6c11564562  commit -Aqm b
  cb9a9f314b8b  commit -Aqm a

Test verbose output

  $ hg reflog -v
  Previous locations of '.':
  cb9a9f314b8b -> 1e6c11564562 * *  up (glob)
  1e6c11564562 -> cb9a9f314b8b * *  up 0 (glob)
  cb9a9f314b8b -> 1e6c11564562 * *  commit -Aqm b (glob)
  000000000000 -> cb9a9f314b8b * *  commit -Aqm a (glob)

  $ hg reflog -v foo
  Previous locations of 'foo':
  cb9a9f314b8b -> 1e6c11564562 * *  up (glob)
  1e6c11564562 -> cb9a9f314b8b * *  book -f foo (glob)
  000000000000 -> 1e6c11564562 * *  book -r tip foo (glob)

Test JSON output

  $ hg reflog -T json
  [
   {
    "command": "up",
    "date": "*", (glob)
    "newhashes": "1e6c11564562",
    "oldhashes": "cb9a9f314b8b",
    "user": "*" (glob)
   },
   {
    "command": "up 0",
    "date": "*", (glob)
    "newhashes": "cb9a9f314b8b",
    "oldhashes": "1e6c11564562",
    "user": "*" (glob)
   },
   {
    "command": "commit -Aqm b",
    "date": "*", (glob)
    "newhashes": "1e6c11564562",
    "oldhashes": "cb9a9f314b8b",
    "user": "*" (glob)
   },
   {
    "command": "commit -Aqm a",
    "date": "*", (glob)
    "newhashes": "cb9a9f314b8b",
    "oldhashes": "000000000000",
    "user": "*" (glob)
   }
  ]

Test pulls

  $ hg init ../repo2
  $ hg push -q -B foo ../repo2
  $ hg strip -q -r . --config extensions.strip=
  $ hg pull -q ../repo2
  $ hg reflog foo
  Previous locations of 'foo':
  1e6c11564562  pull -q ../repo2
  cb9a9f314b8b  strip -q -r .
  1e6c11564562  up
  cb9a9f314b8b  book -f foo
  1e6c11564562  book -r tip foo

Test --commits option

  $ hg reflog --commits
  Previous locations of '.':
  cb9a9f314b8b  strip -q -r .
  changeset:   0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  1e6c11564562  up
  changeset:   1:1e6c11564562
  bookmark:    foo
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  cb9a9f314b8b  up 0
  changeset:   0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  1e6c11564562  commit -Aqm b
  changeset:   1:1e6c11564562
  bookmark:    foo
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  cb9a9f314b8b  commit -Aqm a
  changeset:   0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
Test --commits with JSON output

  $ hg reflog --commits -T json
  [[
   {
    "rev": 0,
    "node": "cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b",
    "branch": "default",
    "phase": "public",
    "user": "test",
    "date": [0, 0],
    "desc": "a",
    "bookmarks": [],
    "tags": [],
    "parents": ["0000000000000000000000000000000000000000"]
   }
  ]
  
   {
    "command": "strip -q -r .",
    "date": "*", (glob)
    "newhashes": "cb9a9f314b8b",
    "oldhashes": "1e6c11564562",
    "user": "*" (glob)
   }[
   {
    "rev": 1,
    "node": "1e6c11564562b4ed919baca798bc4338bd299d6a",
    "branch": "default",
    "phase": "public",
    "user": "test",
    "date": [0, 0],
    "desc": "b",
    "bookmarks": ["foo"],
    "tags": ["tip"],
    "parents": ["cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b"]
   }
  ]
  ,
   {
    "command": "up",
    "date": "*", (glob)
    "newhashes": "1e6c11564562",
    "oldhashes": "cb9a9f314b8b",
    "user": "*" (glob)
   }[
   {
    "rev": 0,
    "node": "cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b",
    "branch": "default",
    "phase": "public",
    "user": "test",
    "date": [0, 0],
    "desc": "a",
    "bookmarks": [],
    "tags": [],
    "parents": ["0000000000000000000000000000000000000000"]
   }
  ]
  ,
   {
    "command": "up 0",
    "date": "*", (glob)
    "newhashes": "cb9a9f314b8b",
    "oldhashes": "1e6c11564562",
    "user": "*" (glob)
   }[
   {
    "rev": 1,
    "node": "1e6c11564562b4ed919baca798bc4338bd299d6a",
    "branch": "default",
    "phase": "public",
    "user": "test",
    "date": [0, 0],
    "desc": "b",
    "bookmarks": ["foo"],
    "tags": ["tip"],
    "parents": ["cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b"]
   }
  ]
  ,
   {
    "command": "commit -Aqm b",
    "date": "*", (glob)
    "newhashes": "1e6c11564562",
    "oldhashes": "cb9a9f314b8b",
    "user": "*" (glob)
   }[
   {
    "rev": 0,
    "node": "cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b",
    "branch": "default",
    "phase": "public",
    "user": "test",
    "date": [0, 0],
    "desc": "a",
    "bookmarks": [],
    "tags": [],
    "parents": ["0000000000000000000000000000000000000000"]
   }
  ]
  ,
   {
    "command": "commit -Aqm a",
    "date": "*", (glob)
    "newhashes": "cb9a9f314b8b",
    "oldhashes": "000000000000",
    "user": "*" (glob)
   }
  ]

Test --commits with -v

  $ hg reflog --commits -v
  Previous locations of '.':
  1e6c11564562 -> cb9a9f314b8b * *  strip -q -r . (glob)
  changeset:   0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  a
  
  
  cb9a9f314b8b -> 1e6c11564562 * *  up (glob)
  changeset:   1:1e6c11564562
  bookmark:    foo
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  b
  
  
  1e6c11564562 -> cb9a9f314b8b * *  up 0 (glob)
  changeset:   0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  a
  
  
  cb9a9f314b8b -> 1e6c11564562 * *  commit -Aqm b (glob)
  changeset:   1:1e6c11564562
  bookmark:    foo
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  b
  
  
  000000000000 -> cb9a9f314b8b * *  commit -Aqm a (glob)
  changeset:   0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  a
  
  

Test --commits with -p

  $ hg reflog --commits -p
  Previous locations of '.':
  cb9a9f314b8b  strip -q -r .
  changeset:   0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  diff -r 000000000000 -r cb9a9f314b8b a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  1e6c11564562  up
  changeset:   1:1e6c11564562
  bookmark:    foo
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  diff -r cb9a9f314b8b -r 1e6c11564562 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -a
  +b
  
  cb9a9f314b8b  up 0
  changeset:   0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  diff -r 000000000000 -r cb9a9f314b8b a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  1e6c11564562  commit -Aqm b
  changeset:   1:1e6c11564562
  bookmark:    foo
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  diff -r cb9a9f314b8b -r 1e6c11564562 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -a
  +b
  
  cb9a9f314b8b  commit -Aqm a
  changeset:   0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  diff -r 000000000000 -r cb9a9f314b8b a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  

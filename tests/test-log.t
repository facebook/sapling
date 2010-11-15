  $ hg init a

  $ cd a
  $ echo a > a
  $ hg ci -Ama -d '1 0'
  adding a

  $ hg cp a b
  $ hg ci -mb -d '2 0'

  $ mkdir dir
  $ hg mv b dir
  $ hg ci -mc -d '3 0'

  $ hg mv a b
  $ echo a > d
  $ hg add d
  $ hg ci -md -d '4 0'

  $ hg mv dir/b e
  $ hg ci -me -d '5 0'

  $ hg log a
  changeset:   0:8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  

-f, directory

  $ hg log -f dir
  abort: cannot follow nonexistent file: "dir"
  [255]

-f, but no args

  $ hg log -f
  changeset:   4:66c1345dc4f9
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  summary:     e
  
  changeset:   3:7c6c671bb7cc
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  summary:     d
  
  changeset:   2:41dd4284081e
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  changeset:   1:784de7cef101
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b
  
  changeset:   0:8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  

one rename

  $ hg log -vf a
  changeset:   0:8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a
  description:
  a
  
  

many renames

  $ hg log -vf e
  changeset:   4:66c1345dc4f9
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  files:       dir/b e
  description:
  e
  
  
  changeset:   2:41dd4284081e
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  files:       b dir/b
  description:
  c
  
  
  changeset:   1:784de7cef101
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  files:       b
  description:
  b
  
  
  changeset:   0:8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a
  description:
  a
  
  


log -pf dir/b

  $ hg log -pf dir/b
  changeset:   2:41dd4284081e
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  summary:     c
  
  diff -r 784de7cef101 -r 41dd4284081e dir/b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/dir/b	Thu Jan 01 00:00:03 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  changeset:   1:784de7cef101
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  summary:     b
  
  diff -r 8580ff50825a -r 784de7cef101 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:02 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  
  changeset:   0:8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  
  diff -r 000000000000 -r 8580ff50825a a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:01 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  

log -vf dir/b

  $ hg log -vf dir/b
  changeset:   2:41dd4284081e
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  files:       b dir/b
  description:
  c
  
  
  changeset:   1:784de7cef101
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  files:       b
  description:
  b
  
  
  changeset:   0:8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a
  description:
  a
  
  


log copies with --copies

  $ hg log -vC --template '{rev} {file_copies}\n'
  4 e (dir/b)
  3 b (a)
  2 dir/b (b)
  1 b (a)
  0 

log copies switch without --copies, with old filecopy template

  $ hg log -v --template '{rev} {file_copies_switch%filecopy}\n'
  4 
  3 
  2 
  1 
  0 

log copies switch with --copies

  $ hg log -vC --template '{rev} {file_copies_switch}\n'
  4 e (dir/b)
  3 b (a)
  2 dir/b (b)
  1 b (a)
  0 


log copies with hardcoded style and with --style=default

  $ hg log -vC -r4
  changeset:   4:66c1345dc4f9
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  files:       dir/b e
  copies:      e (dir/b)
  description:
  e
  
  
  $ hg log -vC -r4 --style=default
  changeset:   4:66c1345dc4f9
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:05 1970 +0000
  files:       dir/b e
  copies:      e (dir/b)
  description:
  e
  
  


log copies, non-linear manifest

  $ hg up -C 3
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg mv dir/b e
  $ echo foo > foo
  $ hg ci -Ame2 -d '6 0'
  adding foo
  created new head
  $ hg log -v --template '{rev} {file_copies}\n' -r 5
  5 e (dir/b)


log copies, execute bit set

  $ chmod +x e
  $ hg ci -me3 -d '7 0'
  $ hg log -v --template '{rev} {file_copies}\n' -r 6
  6 


log -p d

  $ hg log -pv d
  changeset:   3:7c6c671bb7cc
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  files:       a b d
  description:
  d
  
  
  diff -r 41dd4284081e -r 7c6c671bb7cc d
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d	Thu Jan 01 00:00:04 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  


log --removed file

  $ hg log --removed -v a
  changeset:   3:7c6c671bb7cc
  user:        test
  date:        Thu Jan 01 00:00:04 1970 +0000
  files:       a b d
  description:
  d
  
  
  changeset:   0:8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a
  description:
  a
  
  

log --removed revrange file

  $ hg log --removed -v -r0:2 a
  changeset:   0:8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a
  description:
  a
  
  


log --follow tests

  $ hg init ../follow
  $ cd ../follow

  $ echo base > base
  $ hg ci -Ambase -d '1 0'
  adding base

  $ echo r1 >> base
  $ hg ci -Amr1 -d '1 0'
  $ echo r2 >> base
  $ hg ci -Amr2 -d '1 0'

  $ hg up -C 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b1 > b1
  $ hg ci -Amb1 -d '1 0'
  adding b1
  created new head


log -f

  $ hg log -f
  changeset:   3:e62f78d544b4
  tag:         tip
  parent:      1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1
  
  changeset:   1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  
  changeset:   0:67e992f2c4f3
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     base
  


log -f -r 1:tip

  $ hg up -C 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b2 > b2
  $ hg ci -Amb2 -d '1 0'
  adding b2
  created new head
  $ hg log -f -r 1:tip
  changeset:   1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  
  changeset:   2:60c670bf5b30
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r2
  
  changeset:   3:e62f78d544b4
  parent:      1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1
  


log -r .  with two parents

  $ hg up -C 3
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg log -r .
  changeset:   3:e62f78d544b4
  parent:      1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1
  


log -r .  with one parent

  $ hg ci -mm12 -d '1 0'
  $ hg log -r .
  changeset:   5:302e9dd6890d
  tag:         tip
  parent:      3:e62f78d544b4
  parent:      4:ddb82e70d1a1
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12
  

  $ echo postm >> b1
  $ hg ci -Amb1.1 -d'1 0'


log --follow-first

  $ hg log --follow-first
  changeset:   6:2404bbcab562
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
  changeset:   5:302e9dd6890d
  parent:      3:e62f78d544b4
  parent:      4:ddb82e70d1a1
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12
  
  changeset:   3:e62f78d544b4
  parent:      1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1
  
  changeset:   1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  
  changeset:   0:67e992f2c4f3
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     base
  


log -P 2

  $ hg log -P 2
  changeset:   6:2404bbcab562
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
  changeset:   5:302e9dd6890d
  parent:      3:e62f78d544b4
  parent:      4:ddb82e70d1a1
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12
  
  changeset:   4:ddb82e70d1a1
  parent:      0:67e992f2c4f3
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b2
  
  changeset:   3:e62f78d544b4
  parent:      1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1
  


log -r tip -p --git

  $ hg log -r tip -p --git
  changeset:   6:2404bbcab562
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
  diff --git a/b1 b/b1
  --- a/b1
  +++ b/b1
  @@ -1,1 +1,2 @@
   b1
  +postm
  


log -r ""

  $ hg log -r ''
  hg: parse error: empty query
  [255]

log -r <some unknown node id>

  $ hg log -r 1000000000000000000000000000000000000000
  abort: unknown revision '1000000000000000000000000000000000000000'!
  [255]

log -k r1

  $ hg log -k r1
  changeset:   1:3d5bf5654eda
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     r1
  


log -d -1

  $ hg log -d -1


log -p -l2 --color=always

  $ hg --config extensions.color= --config color.mode=ansi \
  >  log -p -l2 --color=always
  \x1b[0;33mchangeset:   6:2404bbcab562\x1b[0m (esc)
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
  \x1b[0;1mdiff -r 302e9dd6890d -r 2404bbcab562 b1\x1b[0m (esc)
  \x1b[0;31;1m--- a/b1	Thu Jan 01 00:00:01 1970 +0000\x1b[0m (esc)
  \x1b[0;32;1m+++ b/b1	Thu Jan 01 00:00:01 1970 +0000\x1b[0m (esc)
  \x1b[0;35m@@ -1,1 +1,2 @@\x1b[0m (esc)
   b1
  \x1b[0;32m+postm\x1b[0m (esc)
  
  \x1b[0;33mchangeset:   5:302e9dd6890d\x1b[0m (esc)
  parent:      3:e62f78d544b4
  parent:      4:ddb82e70d1a1
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     m12
  
  \x1b[0;1mdiff -r e62f78d544b4 -r 302e9dd6890d b2\x1b[0m (esc)
  \x1b[0;31;1m--- /dev/null	Thu Jan 01 00:00:00 1970 +0000\x1b[0m (esc)
  \x1b[0;32;1m+++ b/b2	Thu Jan 01 00:00:01 1970 +0000\x1b[0m (esc)
  \x1b[0;35m@@ -0,0 +1,1 @@\x1b[0m (esc)
  \x1b[0;32m+b2\x1b[0m (esc)
  


log -r tip --stat

  $ hg log -r tip --stat
  changeset:   6:2404bbcab562
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b1.1
  
   b1 |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  

  $ cd ..

  $ hg init usertest
  $ cd usertest

  $ echo a > a
  $ hg ci -A -m "a" -u "User One <user1@example.org>"
  adding a
  $ echo b > b
  $ hg ci -A -m "b" -u "User Two <user2@example.org>"
  adding b

  $ hg log -u "User One <user1@example.org>"
  changeset:   0:29a4c94f1924
  user:        User One <user1@example.org>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ hg log -u "user1" -u "user2"
  changeset:   1:e834b5e69c0e
  tag:         tip
  user:        User Two <user2@example.org>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   0:29a4c94f1924
  user:        User One <user1@example.org>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ hg log -u "user3"

  $ cd ..

  $ hg init branches
  $ cd branches

  $ echo a > a
  $ hg ci -A -m "commit on default"
  adding a
  $ hg branch test
  marked working directory as branch test
  $ echo b > b
  $ hg ci -A -m "commit on test"
  adding b

  $ hg up default
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ hg ci -A -m "commit on default"
  adding c
  $ hg up test
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ hg ci -A -m "commit on test"
  adding c


log -b default

  $ hg log -b default
  changeset:   2:c3a4f03cc9a7
  parent:      0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  changeset:   0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  


log -b test

  $ hg log -b test
  changeset:   3:f5d8de11c2e2
  branch:      test
  tag:         tip
  parent:      1:d32277701ccb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  changeset:   1:d32277701ccb
  branch:      test
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  


log -b dummy

  $ hg log -b dummy
  abort: unknown revision 'dummy'!
  [255]


log -b .

  $ hg log -b .
  changeset:   3:f5d8de11c2e2
  branch:      test
  tag:         tip
  parent:      1:d32277701ccb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  changeset:   1:d32277701ccb
  branch:      test
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  


log -b default -b test

  $ hg log -b default -b test
  changeset:   3:f5d8de11c2e2
  branch:      test
  tag:         tip
  parent:      1:d32277701ccb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  changeset:   2:c3a4f03cc9a7
  parent:      0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  changeset:   1:d32277701ccb
  branch:      test
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  changeset:   0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  


log -b default -b .

  $ hg log -b default -b .
  changeset:   3:f5d8de11c2e2
  branch:      test
  tag:         tip
  parent:      1:d32277701ccb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  changeset:   2:c3a4f03cc9a7
  parent:      0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  changeset:   1:d32277701ccb
  branch:      test
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  changeset:   0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  


log -b . -b test

  $ hg log -b . -b test
  changeset:   3:f5d8de11c2e2
  branch:      test
  tag:         tip
  parent:      1:d32277701ccb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  changeset:   1:d32277701ccb
  branch:      test
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  


log -b 2

  $ hg log -b 2
  changeset:   2:c3a4f03cc9a7
  parent:      0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  changeset:   0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  


log -p --cwd dir (in subdir)

  $ mkdir dir
  $ hg log -p --cwd dir
  changeset:   3:f5d8de11c2e2
  branch:      test
  tag:         tip
  parent:      1:d32277701ccb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  diff -r d32277701ccb -r f5d8de11c2e2 c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  changeset:   2:c3a4f03cc9a7
  parent:      0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  diff -r 24427303d56f -r c3a4f03cc9a7 c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  
  changeset:   1:d32277701ccb
  branch:      test
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on test
  
  diff -r 24427303d56f -r d32277701ccb b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  changeset:   0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  diff -r 000000000000 -r 24427303d56f a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  


log -p -R repo

  $ cd dir
  $ hg log -p -R .. ../a
  changeset:   0:24427303d56f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit on default
  
  diff -r 000000000000 -r 24427303d56f a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  


  $ cd ..
  $ hg init follow2
  $ cd follow2


# Build the following history:
# tip - o - x - o - x - x
#    \                 /
#     o - o - o - x
#      \     /
#         o
#
# Where "o" is a revision containing "foo" and
# "x" is a revision without "foo"

  $ touch init
  $ hg ci -A -m "init, unrelated"
  adding init
  $ echo 'foo' > init
  $ hg ci -m "change, unrelated"
  $ echo 'foo' > foo
  $ hg ci -A -m "add unrelated old foo"
  adding foo
  $ hg rm foo
  $ hg ci -m "delete foo, unrelated"
  $ echo 'related' > foo
  $ hg ci -A -m "add foo, related"
  adding foo

  $ hg up 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch branch
  $ hg ci -A -m "first branch, unrelated"
  adding branch
  created new head
  $ touch foo
  $ hg ci -A -m "create foo, related"
  adding foo
  $ echo 'change' > foo
  $ hg ci -m "change foo, related"

  $ hg up 6
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 'change foo in branch' > foo
  $ hg ci -m "change foo in branch, related"
  created new head
  $ hg merge 7
  merging foo
  warning: conflicts during merge.
  merging foo failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ echo 'merge 1' > foo
  $ hg resolve -m foo
  $ hg ci -m "First merge, related"

  $ hg merge 4
  merging foo
  warning: conflicts during merge.
  merging foo failed!
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ echo 'merge 2' > foo
  $ hg resolve -m foo
  $ hg ci -m "Last merge, related"

  $ hg --config "extensions.graphlog=" glog
  @    changeset:   10:4dae8563d2c5
  |\   tag:         tip
  | |  parent:      9:7b35701b003e
  | |  parent:      4:88176d361b69
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     Last merge, related
  | |
  | o    changeset:   9:7b35701b003e
  | |\   parent:      8:e5416ad8a855
  | | |  parent:      7:87fe3144dcfa
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | | |  summary:     First merge, related
  | | |
  | | o  changeset:   8:e5416ad8a855
  | | |  parent:      6:dc6c325fe5ee
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | | |  summary:     change foo in branch, related
  | | |
  | o |  changeset:   7:87fe3144dcfa
  | |/   user:        test
  | |    date:        Thu Jan 01 00:00:00 1970 +0000
  | |    summary:     change foo, related
  | |
  | o  changeset:   6:dc6c325fe5ee
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     create foo, related
  | |
  | o  changeset:   5:73db34516eb9
  | |  parent:      0:e87515fd044a
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     first branch, unrelated
  | |
  o |  changeset:   4:88176d361b69
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     add foo, related
  | |
  o |  changeset:   3:dd78ae4afb56
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     delete foo, unrelated
  | |
  o |  changeset:   2:c4c64aedf0f7
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     add unrelated old foo
  | |
  o |  changeset:   1:e5faa7440653
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     change, unrelated
  |
  o  changeset:   0:e87515fd044a
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     init, unrelated
  

  $ hg --traceback log -f foo
  changeset:   10:4dae8563d2c5
  tag:         tip
  parent:      9:7b35701b003e
  parent:      4:88176d361b69
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Last merge, related
  
  changeset:   9:7b35701b003e
  parent:      8:e5416ad8a855
  parent:      7:87fe3144dcfa
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     First merge, related
  
  changeset:   8:e5416ad8a855
  parent:      6:dc6c325fe5ee
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo in branch, related
  
  changeset:   7:87fe3144dcfa
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo, related
  
  changeset:   6:dc6c325fe5ee
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     create foo, related
  
  changeset:   4:88176d361b69
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo, related
  

Also check when maxrev < lastrevfilelog

  $ hg --traceback log -f -r4 foo
  changeset:   4:88176d361b69
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo, related
  

Issue2383: hg log showing _less_ differences than hg diff

  $ hg init issue2383
  $ cd issue2383

Create a test repo:

  $ echo a > a
  $ hg ci -Am0
  adding a
  $ echo b > b
  $ hg ci -Am1
  adding b
  $ hg co 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > a
  $ hg ci -m2
  created new head

Merge:

  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

Make sure there's a file listed in the merge to trigger the bug:

  $ echo c > a
  $ hg ci -m3

Two files shown here in diff:

  $ hg diff --rev 2:3
  diff -r b09be438c43a -r 8e07aafe1edc a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -b
  +c
  diff -r b09be438c43a -r 8e07aafe1edc b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b

Diff here should be the same:

  $ hg log -vpr 3
  changeset:   3:8e07aafe1edc
  tag:         tip
  parent:      2:b09be438c43a
  parent:      1:925d80f479bb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  3
  
  
  diff -r b09be438c43a -r 8e07aafe1edc a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -b
  +c
  diff -r b09be438c43a -r 8e07aafe1edc b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b
  
  $ cd ..

'hg log -r rev fn' when last(filelog(fn)) != rev

  $ hg init simplelog; cd simplelog
  $ echo f > a
  $ hg ci -Am'a' -d '0 0'
  adding a
  $ echo f >> a
  $ hg ci -Am'a bis' -d '1 0'

  $ hg log -r0 a
  changeset:   0:9f758d63dcde
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  

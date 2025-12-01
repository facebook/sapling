# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ setup_common_config

  $ cd $TESTTMP

setup repo

  $ quiet testtool_drawdag -R repo <<EOF
  > A
  > # modify: A "a" "a file content"
  > # delete: A "A"
  > # bookmark: A master_bookmark
  > # message: A "a"
  > # author: A test
  > EOF

start mononoke

  $ start_and_wait_for_mononoke_server

setup two repos: one will be used to push from, another will be used
to pull these pushed commits

  $ hg clone -q mono:repo repo2
  $ hg clone -q mono:repo repo3
  $ cd repo2
  $ hg pull ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo

BEGIN Creation of new commits

create new commits in repo2 and check that they are seen as outgoing

  $ mkdir b_dir
  $ echo "new a file content" > a
  $ echo "b file content" > b_dir/b
  $ hg add b_dir/b
  $ hg ci -mb

  $ echo "updated b file content" > b_dir/b
  $ mkdir c_dir
  $ echo "c file content" > c_dir/c
  $ hg add c_dir/c
  $ hg ci -mc

create a commit that makes identical change to file b

  $ hg update '.^'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "updated b file content" > b_dir/b
  $ mkdir d_dir
  $ echo "d file content" > d_dir/d
  $ hg add d_dir/d
  $ hg ci -md

create a commit that reverts files a and b to older version

  $ echo "a file content" > a
  $ echo "b file content" > b_dir/b
  $ hg ci -me

create a commit that sets content of some files to content of other files

  $ echo "b file content" > a
  $ echo "a file content" > b_dir/b
  $ mkdir c_dir
  $ echo "a file content" > c_dir/c
  $ hg add c_dir/c
  $ echo "b file content" > d_dir/d
  $ hg ci -mf

create a commit that renames, copy and deletes some files

  $ hg rm b_dir/b
  $ hg mv a b_dir/b
  $ mkdir e_dir
  $ hg mv c_dir/c e_dir/e
  $ mkdir a_dir
  $ hg mv d_dir/d a_dir/a
  $ echo "a file content" > a_dir/a
  $ hg cp a_dir/a b_dir/a_bis
  $ hg ci -mg

END Creation of new commits

move master bookmarks

  $ hg bookmark -f master_bookmark -r 'tip'

  $ hg log -r "reverse(all())" --stat
  commit:      164fa3d7a55c
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
   (re)
   b_dir/b |  2 +-
   c_dir/c |  1 +
   2 files changed, 2 insertions(+), 1 deletions(-)
  
  commit:      d57b20e747f8
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     g
  
   a           |  1 -
   a_dir/a     |  1 +
   b_dir/a_bis |  1 +
   b_dir/b     |  2 +-
   c_dir/c     |  1 -
   d_dir/d     |  1 -
   e_dir/e     |  1 +
   7 files changed, 4 insertions(+), 4 deletions(-)
  
  commit:      c1872d432eba
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  
   a       |  2 +-
   b_dir/b |  2 +-
   c_dir/c |  1 +
   d_dir/d |  2 +-
   4 files changed, 4 insertions(+), 3 deletions(-)
  
  commit:      20c500549573
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  
   a       |  2 +-
   b_dir/b |  2 +-
   2 files changed, 2 insertions(+), 2 deletions(-)
  
  commit:      17a13ec35321
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     d
  
   b_dir/b |  2 +-
   d_dir/d |  1 +
   2 files changed, 2 insertions(+), 1 deletions(-)
  
  commit:      73a82cfa87df
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
   a       |  2 +-
   b_dir/b |  1 +
   2 files changed, 2 insertions(+), 1 deletions(-)
  
  commit:      c0fe33181ed1
  bookmark:    remote/master_bookmark
  hoistedname: master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
   a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
   (re)

push to Mononoke

  $ hg push --force --debug --allow-anon
  tracking on None {}
  pushing to mono:repo
  sending hello command
  sending clienttelemetry command
  query 1; heads
  searching for changes
  local heads: 2; remote heads: 1 (explicit: 0); initial common: 1
  sampling from both directions (2 of 2)
  sampling undecided commits (4 of 4)
  query 2; still undecided: 4, sample size is: 4
  2 total queries in 0.0000s
  checking for updated bookmarks
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  6 changesets found
  list of changesets:
  73a82cfa87dfc23097b4a429eac744ec99394b30
  17a13ec3532124b3c071bdac0c3c32b3c36f1130
  20c50054957345c7c7c8a8ec480c310134c24e87
  c1872d432eba363d07c2abebb03ae55d1c6c0f95
  d57b20e747f87b1a264bd3b58dbaea2dbc154180
  164fa3d7a55cf68a4dafc6383ac9d3cad3d72f56
  sending unbundle command
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  bundle2-input-bundle: 1 params no-transaction
  bundle2-input-part: "reply:changegroup" (params: 2 mandatory) supported
  bundle2-input-part: "reply:pushkey" (params: 2 mandatory) supported
  bundle2-input-bundle: 1 parts total
  updating bookmark master_bookmark
  preparing listkeys for "bookmarks" with pattern "['master_bookmark']"
  sending listkeyspatterns command
  received listkey for "bookmarks": 56 bytes

Now pull what was just pushed

  $ cd ../repo3
  $ hg log -r "reverse(all())" --stat
  commit:      c0fe33181ed1
  bookmark:    remote/master_bookmark
  hoistedname: master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
   (re)
   a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
   (re)
  $ hg pull -q

Because the revision numbers are assigned nondeterministically we cannot
compare output of the entire tree. Instead we compare only linear histories

  $ hg log --graph --template '{node} {bookmarks}' -r "::164fa3d7a55cf68a4dafc6383ac9d3cad3d72f56"
  pulling '164fa3d7a55cf68a4dafc6383ac9d3cad3d72f56' from 'mono:repo'
  o  164fa3d7a55cf68a4dafc6383ac9d3cad3d72f56
  │
  o  73a82cfa87dfc23097b4a429eac744ec99394b30
  │
  @  c0fe33181ed12aadb7078edd469d1a5ee3da8f29
  
  $ hg log --graph --template '{node} {bookmarks}' -r "::d57b20e747f87b1a264bd3b58dbaea2dbc154180"
  o  d57b20e747f87b1a264bd3b58dbaea2dbc154180
  │
  o  c1872d432eba363d07c2abebb03ae55d1c6c0f95
  │
  o  20c50054957345c7c7c8a8ec480c310134c24e87
  │
  o  17a13ec3532124b3c071bdac0c3c32b3c36f1130
  │
  o  73a82cfa87dfc23097b4a429eac744ec99394b30
  │
  @  c0fe33181ed12aadb7078edd469d1a5ee3da8f29
  
This last step is verifying every commit one by one, it is done in a single
command, but the output of this command is long

  $ for commit in `hg log --template '{node} ' -r 'c0fe33181ed1::d57b20e747f87b1a264bd3b58dbaea2dbc154180'` 164fa3d7a55cf68a4dafc6383ac9d3cad3d72f56; do \
  $ if [ "`hg export -R $TESTTMP/repo2 ${commit}`" == "`hg export ${commit} 2> /dev/null`" ]; then echo "${commit} comparison SUCCESS"; fi; hg export ${commit}; echo; echo; done
  c0fe33181ed12aadb7078edd469d1a5ee3da8f29 comparison SUCCESS
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID c0fe33181ed12aadb7078edd469d1a5ee3da8f29
  # Parent  0000000000000000000000000000000000000000
  a
   (re)
  diff -r 000000000000 -r c0fe33181ed1 a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a file content
  \ No newline at end of file
   (re)
  
  73a82cfa87dfc23097b4a429eac744ec99394b30 comparison SUCCESS
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 73a82cfa87dfc23097b4a429eac744ec99394b30
  # Parent  c0fe33181ed12aadb7078edd469d1a5ee3da8f29
  b
  
  diff -r c0fe33181ed1 -r 73a82cfa87df a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -a file content
  \ No newline at end of file
  +new a file content
  diff -r c0fe33181ed1 -r 73a82cfa87df b_dir/b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b file content
  
  
  17a13ec3532124b3c071bdac0c3c32b3c36f1130 comparison SUCCESS
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 17a13ec3532124b3c071bdac0c3c32b3c36f1130
  # Parent  73a82cfa87dfc23097b4a429eac744ec99394b30
  d
  
  diff -r 73a82cfa87df -r 17a13ec35321 b_dir/b
  --- a/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -b file content
  +updated b file content
  diff -r 73a82cfa87df -r 17a13ec35321 d_dir/d
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d_dir/d	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +d file content
  
  
  20c50054957345c7c7c8a8ec480c310134c24e87 comparison SUCCESS
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 20c50054957345c7c7c8a8ec480c310134c24e87
  # Parent  17a13ec3532124b3c071bdac0c3c32b3c36f1130
  e
  
  diff -r 17a13ec35321 -r 20c500549573 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -new a file content
  +a file content
  diff -r 17a13ec35321 -r 20c500549573 b_dir/b
  --- a/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -updated b file content
  +b file content
  
  
  c1872d432eba363d07c2abebb03ae55d1c6c0f95 comparison SUCCESS
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID c1872d432eba363d07c2abebb03ae55d1c6c0f95
  # Parent  20c50054957345c7c7c8a8ec480c310134c24e87
  f
  
  diff -r 20c500549573 -r c1872d432eba a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -a file content
  +b file content
  diff -r 20c500549573 -r c1872d432eba b_dir/b
  --- a/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -b file content
  +a file content
  diff -r 20c500549573 -r c1872d432eba c_dir/c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c_dir/c	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a file content
  diff -r 20c500549573 -r c1872d432eba d_dir/d
  --- a/d_dir/d	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d_dir/d	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -d file content
  +b file content
  
  
  d57b20e747f87b1a264bd3b58dbaea2dbc154180 comparison SUCCESS
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID d57b20e747f87b1a264bd3b58dbaea2dbc154180
  # Parent  c1872d432eba363d07c2abebb03ae55d1c6c0f95
  g
  
  diff -r c1872d432eba -r d57b20e747f8 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -b file content
  diff -r c1872d432eba -r d57b20e747f8 a_dir/a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a_dir/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a file content
  diff -r c1872d432eba -r d57b20e747f8 b_dir/a_bis
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b_dir/a_bis	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a file content
  diff -r c1872d432eba -r d57b20e747f8 b_dir/b
  --- a/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -a file content
  +b file content
  diff -r c1872d432eba -r d57b20e747f8 c_dir/c
  --- a/c_dir/c	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a file content
  diff -r c1872d432eba -r d57b20e747f8 d_dir/d
  --- a/d_dir/d	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -b file content
  diff -r c1872d432eba -r d57b20e747f8 e_dir/e
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/e_dir/e	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a file content
  
  
  164fa3d7a55cf68a4dafc6383ac9d3cad3d72f56 comparison SUCCESS
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 164fa3d7a55cf68a4dafc6383ac9d3cad3d72f56
  # Parent  73a82cfa87dfc23097b4a429eac744ec99394b30
  c
  
  diff -r 73a82cfa87df -r 164fa3d7a55c b_dir/b
  --- a/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -b file content
  +updated b file content
  diff -r 73a82cfa87df -r 164fa3d7a55c c_dir/c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c_dir/c	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +c file content
  
  

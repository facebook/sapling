  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ setup_common_config

  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

setup master bookmarks

  $ hg bookmark master_bookmark -r 'tip'
  $ hg bookmark master_bookmark2 -r 'tip'

verify content
  $ hg log
  changeset:   0:0e7ec5675652
  bookmark:    master_bookmark
  bookmark:    master_bookmark2
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  

  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

setup two repos: one will be used to push from, another will be used
to pull these pushed commits

  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo3
  $ cd repo2
  $ hg pull ../repo-hg
  pulling from ../repo-hg
  searching for changes
  no changes found

start mononoke

  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

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
  $ hg bookmark -f master_bookmark2 -r 'f40c09205504'

  $ hg log -r "reverse(all())" --stat
  changeset:   6:634de738bb0f
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     g
   (re)
   a           |  1 -
   a_dir/a     |  1 +
   b_dir/a_bis |  1 +
   b_dir/b     |  2 +-
   c_dir/c     |  1 -
   d_dir/d     |  1 -
   e_dir/e     |  1 +
   7 files changed, 4 insertions(+), 4 deletions(-)
  
  changeset:   5:8315ea53ef41
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
  
   a       |  2 +-
   b_dir/b |  2 +-
   c_dir/c |  1 +
   d_dir/d |  2 +-
   4 files changed, 4 insertions(+), 3 deletions(-)
  
  changeset:   4:30da5bf63484
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  
   a       |  2 +-
   b_dir/b |  2 +-
   2 files changed, 2 insertions(+), 2 deletions(-)
  
  changeset:   3:fbd6b221382e
  parent:      1:bb0985934a0f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     d
  
   b_dir/b |  2 +-
   d_dir/d |  1 +
   2 files changed, 2 insertions(+), 1 deletions(-)
  
  changeset:   2:f40c09205504
  bookmark:    master_bookmark2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
   b_dir/b |  2 +-
   c_dir/c |  1 +
   2 files changed, 2 insertions(+), 1 deletions(-)
  
  changeset:   1:bb0985934a0f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
   a       |  2 +-
   b_dir/b |  1 +
   2 files changed, 2 insertions(+), 1 deletions(-)
  
  changeset:   0:0e7ec5675652
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
   a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
   (re)

  $ hgmn outgoing ssh://user@dummy/repo
  comparing with ssh://user@dummy/repo
  searching for changes
  changeset:   1:bb0985934a0f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
   (re)
  changeset:   2:f40c09205504
  bookmark:    master_bookmark2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
   (re)
  changeset:   3:fbd6b221382e
  parent:      1:bb0985934a0f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     d
   (re)
  changeset:   4:30da5bf63484
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
   (re)
  changeset:   5:8315ea53ef41
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     f
   (re)
  changeset:   6:634de738bb0f
  bookmark:    master_bookmark
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     g
   (re)

push to Mononoke

  $ hgmn push --force --config treemanifest.treeonly=True --debug ssh://user@dummy/repo
  pushing to ssh://user@dummy/repo
  running * 'user@dummy' '$TESTTMP/mononoke_hgcli -R repo serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  sending clienttelemetry command
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 0 bytes
  checking for updated bookmarks
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 115 bytes
  6 changesets found
  list of changesets:
  bb0985934a0f8a493887892173b68940ceb40b4f
  f40c09205504d8410f8c8679bf7a85fef25f9337
  fbd6b221382efa5d5bc53130cdaccf06e04c97d3
  30da5bf63484d2d6572edafb3ea211c17cd8c005
  8315ea53ef41d34f56232c88669cc80225b6e66d
  634de738bb0ff135e32d48567718fb9d7dedf575
  sending unbundle command
  bundle2-output-bundle: "HG20", 5 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  bundle2-input-bundle: 1 params no-transaction
  bundle2-input-part: "reply:changegroup" (params: 2 mandatory) supported
  bundle2-input-part: "reply:pushkey" (params: 2 mandatory) supported
  bundle2-input-part: "reply:pushkey" (params: 2 mandatory) supported
  bundle2-input-bundle: 2 parts total
  updating bookmark master_bookmark
  updating bookmark master_bookmark2
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 0 bytes

Now pull what was just pushed

  $ cd ../repo3
  $ hgmn log -r "reverse(all())" --stat
  changeset:   0:0e7ec5675652
  bookmark:    master_bookmark
  bookmark:    master_bookmark2
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
   (re)
   a |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
   (re)
  $ hgmn pull -q

Because the revision numbers are assigned nondeterministically we cannot
compare output of the entire tree. Instead we compare only linear histories

  $ hgmn log --graph --template '{node} {bookmarks}' -r "::f40c09205504"
  o  f40c09205504d8410f8c8679bf7a85fef25f9337 master_bookmark2
  |
  o  bb0985934a0f8a493887892173b68940ceb40b4f
  |
  @  0e7ec5675652a04069cbf976a42e45b740f3243c
   (re)
  $ hgmn log --graph --template '{node} {bookmarks}' -r "::634de738bb0f"
  o  634de738bb0ff135e32d48567718fb9d7dedf575 master_bookmark
  |
  o  8315ea53ef41d34f56232c88669cc80225b6e66d
  |
  o  30da5bf63484d2d6572edafb3ea211c17cd8c005
  |
  o  fbd6b221382efa5d5bc53130cdaccf06e04c97d3
  |
  o  bb0985934a0f8a493887892173b68940ceb40b4f
  |
  @  0e7ec5675652a04069cbf976a42e45b740f3243c
   (re)
This last step is verifying every commit one by one, it is done in a single
command, but the output of this command is long

  $ for commit in `hg log --template '{node} ' -r '0e7ec567::634de738'` f40c09205504d8410f8c8679bf7a85fef25f9337; do \
  $ if [ "`hg export -R $TESTTMP/repo2 ${commit}`" == "`hgmn export ${commit} 2> /dev/null`" ]; then echo "${commit} comparison SUCCESS"; fi; hgmn export ${commit}; echo; echo; done
  0e7ec5675652a04069cbf976a42e45b740f3243c comparison SUCCESS
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 0e7ec5675652a04069cbf976a42e45b740f3243c
  # Parent  0000000000000000000000000000000000000000
  a
   (re)
  diff -r 000000000000 -r 0e7ec5675652 a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a file content
   (re)
   (re)
  bb0985934a0f8a493887892173b68940ceb40b4f comparison SUCCESS
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID bb0985934a0f8a493887892173b68940ceb40b4f
  # Parent  0e7ec5675652a04069cbf976a42e45b740f3243c
  b
  
  diff -r 0e7ec5675652 -r bb0985934a0f a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -a file content
  +new a file content
  diff -r 0e7ec5675652 -r bb0985934a0f b_dir/b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b file content
  
  
  fbd6b221382efa5d5bc53130cdaccf06e04c97d3 comparison SUCCESS
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID fbd6b221382efa5d5bc53130cdaccf06e04c97d3
  # Parent  bb0985934a0f8a493887892173b68940ceb40b4f
  d
  
  diff -r bb0985934a0f -r fbd6b221382e b_dir/b
  --- a/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -b file content
  +updated b file content
  diff -r bb0985934a0f -r fbd6b221382e d_dir/d
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d_dir/d	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +d file content
  
  
  30da5bf63484d2d6572edafb3ea211c17cd8c005 comparison SUCCESS
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 30da5bf63484d2d6572edafb3ea211c17cd8c005
  # Parent  fbd6b221382efa5d5bc53130cdaccf06e04c97d3
  e
  
  diff -r fbd6b221382e -r 30da5bf63484 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -new a file content
  +a file content
  diff -r fbd6b221382e -r 30da5bf63484 b_dir/b
  --- a/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -updated b file content
  +b file content
  
  
  8315ea53ef41d34f56232c88669cc80225b6e66d comparison SUCCESS
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 8315ea53ef41d34f56232c88669cc80225b6e66d
  # Parent  30da5bf63484d2d6572edafb3ea211c17cd8c005
  f
  
  diff -r 30da5bf63484 -r 8315ea53ef41 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -a file content
  +b file content
  diff -r 30da5bf63484 -r 8315ea53ef41 b_dir/b
  --- a/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -b file content
  +a file content
  diff -r 30da5bf63484 -r 8315ea53ef41 c_dir/c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c_dir/c	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a file content
  diff -r 30da5bf63484 -r 8315ea53ef41 d_dir/d
  --- a/d_dir/d	Thu Jan 01 00:00:00 1970 +0000
  +++ b/d_dir/d	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -d file content
  +b file content
  
  
  634de738bb0ff135e32d48567718fb9d7dedf575 comparison SUCCESS
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 634de738bb0ff135e32d48567718fb9d7dedf575
  # Parent  8315ea53ef41d34f56232c88669cc80225b6e66d
  g
  
  diff -r 8315ea53ef41 -r 634de738bb0f a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -b file content
  diff -r 8315ea53ef41 -r 634de738bb0f a_dir/a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a_dir/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a file content
  diff -r 8315ea53ef41 -r 634de738bb0f b_dir/a_bis
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b_dir/a_bis	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a file content
  diff -r 8315ea53ef41 -r 634de738bb0f b_dir/b
  --- a/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -a file content
  +b file content
  diff -r 8315ea53ef41 -r 634de738bb0f c_dir/c
  --- a/c_dir/c	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -a file content
  diff -r 8315ea53ef41 -r 634de738bb0f d_dir/d
  --- a/d_dir/d	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -b file content
  diff -r 8315ea53ef41 -r 634de738bb0f e_dir/e
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/e_dir/e	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a file content
  
  
  f40c09205504d8410f8c8679bf7a85fef25f9337 comparison SUCCESS
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID f40c09205504d8410f8c8679bf7a85fef25f9337
  # Parent  bb0985934a0f8a493887892173b68940ceb40b4f
  c
  
  diff -r bb0985934a0f -r f40c09205504 b_dir/b
  --- a/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b_dir/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -b file content
  +updated b file content
  diff -r bb0985934a0f -r f40c09205504 c_dir/c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c_dir/c	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +c file content
  
  

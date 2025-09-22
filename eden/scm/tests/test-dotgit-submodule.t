#require git no-windows no-eden

  $ . $TESTDIR/git.sh
  $ setconfig diff.git=1

Prepare git repo with submodules

  $ git init -q -b main sub
  $ cd sub
  $ touch a
  $ sl commit -m 'Add a' -A a
  $ echo 1 >> a
  $ sl commit -m 'Modify a' -A a

  $ cd
  $ git init -q -b main parent
  $ cd parent
  $ git submodule --quiet add -b main ../sub
  $ git commit -qm 'add .gitmodules'

Status does not contain uninitialized submodule (cgit behavior):

  $ mv sub/.git "$TESTTMP/subgit"  # make it "uninitialized"
  $ sl status
  $ mv "$TESTTMP/subgit" sub/.git

Status does not contain submodule if submodule is not changed:

  $ touch b
  $ sl status
  ? b

Status and diff can include submodule:

  $ cd sub
  $ git checkout -q 'HEAD^'
  $ cd ..

  $ sl status
  M sub
  ? b

  $ sl diff
  diff --git a/sub b/sub
  --- a/sub
  +++ b/sub
  @@ -1,1 +1,1 @@
  -Subproject commit 838d36ce8147047ed2fb694a88ea81cdfa5041b0
  +Subproject commit 7e03c5d593048a97b91470d7c33dc07e007aa5a4

"debugexportstack -wdir()" works too:

  $ sl debugexportstack -r 'wdir()' --config paths.default=file://$TESTTMP/non-existed
  [{"author": "test <test@example.org>", "date": [1167609610.0, 0], "immutable": false, "node": "377b5a034be6956da87d97624088bdf079c1fc05", "relevantFiles": {"sub": {"data": "Subproject commit 838d36ce8147047ed2fb694a88ea81cdfa5041b0\n", "flags": "m"}}, "requested": false, "text": "add .gitmodules\n"}, {"author": "test", "date": [0, 0], "files": {"sub": {"data": "Subproject commit 7e03c5d593048a97b91470d7c33dc07e007aa5a4\n", "flags": "m"}}, "immutable": false, "node": "ffffffffffffffffffffffffffffffffffffffff", "parents": ["377b5a034be6956da87d97624088bdf079c1fc05"], "requested": true, "text": ""}]

Status from submodule:

  $ cd sub
  $ touch c
  $ sl status
  ? c

Committing from submodule:

  $ sl add c
  $ sl commit -m c

Checking out from submodule:

  $ sl prev
  update complete
  [7e03c5] Add a

  $ sl status

Committing from parent repo:

  $ cd ~/parent
  $ sl status sub
  M sub

  $ sl status sub --config submodule.active-sub=false

  $ sl commit -m 'Modify submodule'

  $ sl status sub

  $ sl log -r . -p
  commit:      d59fa9b13e55
  user:        test <>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Modify submodule
  
  diff --git a/sub b/sub
  --- a/sub
  +++ b/sub
  @@ -1,1 +1,1 @@
  -Subproject commit 838d36ce8147047ed2fb694a88ea81cdfa5041b0
  +Subproject commit 7e03c5d593048a97b91470d7c33dc07e007aa5a4

Cloning a repo with submodules, but without initializing them:

  $ cd
  $ git clone -q parent cloned-without-subm
  $ cd cloned-without-subm

- Submodules are inactive, and absent from status
  $ sl status

(bad: should skip or create the submodule on demand, not crash)
  $ sl go '.^'
  abort: $ENOENT$: $TESTTMP/cloned-without-subm/sub/.git/sl
  [255]

Cloning a repo with submodules recursively:

  $ cd
  $ git clone -q --recursive parent cloned-with-subm
  $ cd cloned-with-subm

  $ sl status

  $ sl go -q '.^'
  $ sl st

Pulling a submodule:

- First, create new commits in the parent repo:
  $ cd ~/parent/sub
  $ sl go -q tip
  $ echo 33 >> c
  $ sl commit -m 'Edit inside submodule'
  $ cd ..
  $ sl commit -m 'Modify submodule again'

- Then, try to pull and checkout:
  $ cd ~/cloned-with-subm
(bad: should not crash)
  $ sl pull
  pulling from $TESTTMP/parent
  From $TESTTMP/parent
   * [new ref]         6c516db950c80a0c163308e8f64f0727b390be35 -> remote/main
  Fetching submodule sub
  fatal: git upload-pack: not our ref 0abf75b119a00371030b88df96abed7e949f63cb
  fatal: remote error: upload-pack: not our ref 0abf75b119a00371030b88df96abed7e949f63cb
  Errors during submodule fetch:
  	sub

(bad: checkout should create sub/c "33")
  $ sl go -q 'next()'
  $ cat sub/c
  cat: sub/c: $ENOENT$
  [1]

  $ "$TESTDIR/hghave" execbit || exit 80

  $ hg init

Setup:

  $ echo a >> a
  $ hg ci -Am 'base'
  adding a

Refuse to amend public csets:

  $ hg phase -r . -p
  $ hg ci --amend
  abort: cannot amend public changesets
  [255]
  $ hg phase -r . -f -d

  $ echo a >> a
  $ hg ci -Am 'base1'

Nothing to amend:

  $ hg ci --amend
  nothing changed
  [1]

Amending changeset with changes in working dir:

  $ echo a >> a
  $ hg ci --amend -m 'amend base1'
  saved backup bundle to $TESTTMP/.hg/strip-backup/489edb5b847d-amend-backup.hg
  $ hg diff -c .
  diff -r ad120869acf0 -r 9cd25b479c51 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,3 @@
   a
  +a
  +a
  $ hg log
  changeset:   1:9cd25b479c51
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     amend base1
  
  changeset:   0:ad120869acf0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     base
  

Add new file:

  $ echo b > b
  $ hg ci --amend -Am 'amend base1 new file'
  adding b
  saved backup bundle to $TESTTMP/.hg/strip-backup/9cd25b479c51-amend-backup.hg

Remove file that was added in amended commit:

  $ hg rm b
  $ hg ci --amend -m 'amend base1 remove new file'
  saved backup bundle to $TESTTMP/.hg/strip-backup/e2bb3ecffd2f-amend-backup.hg

  $ hg cat b
  b: no such file in rev 664a9b2d60cd
  [1]

No changes, just a different message:

  $ hg ci -v --amend -m 'no changes, new message'
  amending changeset 664a9b2d60cd
  copying changeset 664a9b2d60cd to ad120869acf0
  a
  stripping amended changeset 664a9b2d60cd
  1 changesets found
  saved backup bundle to $TESTTMP/.hg/strip-backup/664a9b2d60cd-amend-backup.hg
  1 changesets found
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  committed changeset 1:ea6e356ff2ad
  $ hg diff -c .
  diff -r ad120869acf0 -r ea6e356ff2ad a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,3 @@
   a
  +a
  +a
  $ hg log
  changeset:   1:ea6e356ff2ad
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     no changes, new message
  
  changeset:   0:ad120869acf0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     base
  

Disable default date on commit so when -d isn't given, the old date is preserved:

  $ echo '[defaults]' >> $HGRCPATH
  $ echo 'commit=' >> $HGRCPATH

Test -u/-d:

  $ hg ci --amend -u foo -d '1 0'
  saved backup bundle to $TESTTMP/.hg/strip-backup/ea6e356ff2ad-amend-backup.hg
  $ echo a >> a
  $ hg ci --amend -u foo -d '1 0'
  saved backup bundle to $TESTTMP/.hg/strip-backup/377b91ce8b56-amend-backup.hg
  $ hg log -r .
  changeset:   1:2c94e4a5756f
  tag:         tip
  user:        foo
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     no changes, new message
  

Open editor with old commit message if a message isn't given otherwise:

  $ cat > editor << '__EOF__'
  > #!/bin/sh
  > cat $1
  > echo "another precious commit message" > "$1"
  > __EOF__
  $ chmod +x editor
  $ HGEDITOR="'`pwd`'"/editor hg commit --amend -v
  amending changeset 2c94e4a5756f
  copying changeset 2c94e4a5756f to ad120869acf0
  no changes, new message
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: foo
  HG: branch 'default'
  HG: changed a
  a
  stripping amended changeset 2c94e4a5756f
  1 changesets found
  saved backup bundle to $TESTTMP/.hg/strip-backup/2c94e4a5756f-amend-backup.hg
  1 changesets found
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  committed changeset 1:ffb49186f961

Same, but with changes in working dir (different code path):

  $ echo a >> a
  $ HGEDITOR="'`pwd`'"/editor hg commit --amend -v
  amending changeset ffb49186f961
  another precious commit message
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: foo
  HG: branch 'default'
  HG: changed a
  a
  copying changeset 27f3aacd3011 to ad120869acf0
  a
  stripping intermediate changeset 27f3aacd3011
  stripping amended changeset ffb49186f961
  2 changesets found
  saved backup bundle to $TESTTMP/.hg/strip-backup/ffb49186f961-amend-backup.hg
  1 changesets found
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  committed changeset 1:fb6cca43446f

  $ rm editor
  $ hg log -r .
  changeset:   1:fb6cca43446f
  tag:         tip
  user:        foo
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     another precious commit message
  

Moving bookmarks, preserve active bookmark:

  $ hg book book1
  $ hg book book2
  $ hg ci --amend -m 'move bookmarks'
  saved backup bundle to $TESTTMP/.hg/strip-backup/fb6cca43446f-amend-backup.hg
  $ hg book
     book1                     1:0cf1c7a51bcf
   * book2                     1:0cf1c7a51bcf
  $ echo a >> a
  $ hg ci --amend -m 'move bookmarks'
  saved backup bundle to $TESTTMP/.hg/strip-backup/0cf1c7a51bcf-amend-backup.hg
  $ hg book
     book1                     1:7344472bd951
   * book2                     1:7344472bd951

  $ echo '[defaults]' >> $HGRCPATH
  $ echo "commit=-d '0 0'" >> $HGRCPATH

Moving branches:

  $ hg branch foo
  marked working directory as branch foo
  (branches are permanent and global, did you want a bookmark?)
  $ echo a >> a
  $ hg ci -m 'branch foo'
  $ hg branch default -f
  marked working directory as branch default
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci --amend -m 'back to default'
  saved backup bundle to $TESTTMP/.hg/strip-backup/1661ca36a2db-amend-backup.hg
  $ hg branches
  default                        2:f24ee5961967

Close branch:

  $ hg up -q 0
  $ echo b >> b
  $ hg branch foo
  marked working directory as branch foo
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -Am 'fork'
  adding b
  $ echo b >> b
  $ hg ci -mb
  $ hg ci --amend --close-branch -m 'closing branch foo'
  saved backup bundle to $TESTTMP/.hg/strip-backup/c962248fa264-amend-backup.hg

Same thing, different code path:

  $ echo b >> b
  $ hg ci -m 'reopen branch'
  reopening closed branch head 4
  $ echo b >> b
  $ hg ci --amend --close-branch
  saved backup bundle to $TESTTMP/.hg/strip-backup/5e302dcc12b8-amend-backup.hg
  $ hg branches
  default                        2:f24ee5961967

Refuse to amend merges:

  $ hg up -q default
  $ hg merge foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci --amend
  abort: cannot amend while merging
  [255]
  $ hg ci -m 'merge'
  $ hg ci --amend
  abort: cannot amend merge changesets
  [255]

Follow copies/renames:

  $ hg mv b c
  $ hg ci -m 'b -> c'
  $ hg mv c d
  $ hg ci --amend -m 'b -> d'
  saved backup bundle to $TESTTMP/.hg/strip-backup/9c207120aa98-amend-backup.hg
  $ hg st --rev '.^' --copies d
  A d
    b
  $ hg cp d e
  $ hg ci -m 'e = d'
  $ hg cp e f
  $ hg ci --amend -m 'f = d'
  saved backup bundle to $TESTTMP/.hg/strip-backup/fda2b3b27b22-amend-backup.hg
  $ hg st --rev '.^' --copies f
  A f
    d

  $ mv f f.orig
  $ hg rm -A f
  $ hg ci -m removef
  $ hg cp a f
  $ mv f.orig f
  $ hg ci --amend -m replacef
  saved backup bundle to $TESTTMP/.hg/strip-backup/0ce2c92dc50d-amend-backup.hg
  $ hg st --change . --copies
  M f
  $ hg log -r . --template "{file_copies}\n"
  f (a)

Can't rollback an amend:

  $ hg rollback
  no rollback information available
  [1]

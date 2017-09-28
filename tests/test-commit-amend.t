  $ cat << EOF >> $HGRCPATH
  > [format]
  > usegeneraldelta=yes
  > EOF

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

  $ hg ci --amend -m 'base1'
  nothing changed
  [1]

  $ cat >> $HGRCPATH <<EOF
  > [hooks]
  > pretxncommit.foo = sh -c "echo \\"pretxncommit \$HG_NODE\\"; hg id -r \$HG_NODE"
  > EOF

Amending changeset with changes in working dir:
(and check that --message does not trigger an editor)

  $ echo a >> a
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend -m 'amend base1'
  pretxncommit 43f1ba15f28a50abf0aae529cf8a16bfced7b149
  43f1ba15f28a tip
  saved backup bundle to $TESTTMP/.hg/strip-backup/489edb5b847d-5ab4f721-amend.hg (glob)
  $ echo 'pretxncommit.foo = ' >> $HGRCPATH
  $ hg diff -c .
  diff -r ad120869acf0 -r 43f1ba15f28a a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,3 @@
   a
  +a
  +a
  $ hg log
  changeset:   1:43f1ba15f28a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     amend base1
  
  changeset:   0:ad120869acf0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     base
  

Check proper abort for empty message

  $ cat > editor.sh << '__EOF__'
  > #!/bin/sh
  > echo "" > "$1"
  > __EOF__

Update the existing file to ensure that the dirstate is not in pending state
(where the status of some files in the working copy is not known yet). This in
turn ensures that when the transaction is aborted due to an empty message during
the amend, there should be no rollback.
  $ echo a >> a

  $ echo b > b
  $ hg add b
  $ hg summary
  parent: 1:43f1ba15f28a tip
   amend base1
  branch: default
  commit: 1 modified, 1 added, 1 unknown
  update: (current)
  phases: 2 draft
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend
  abort: empty commit message
  [255]
  $ hg summary
  parent: 1:43f1ba15f28a tip
   amend base1
  branch: default
  commit: 1 modified, 1 added, 1 unknown
  update: (current)
  phases: 2 draft

Add new file along with modified existing file:
  $ hg ci --amend -m 'amend base1 new file'
  saved backup bundle to $TESTTMP/.hg/strip-backup/43f1ba15f28a-007467c2-amend.hg (glob)

Remove file that was added in amended commit:
(and test logfile option)
(and test that logfile option do not trigger an editor)

  $ hg rm b
  $ echo 'amend base1 remove new file' > ../logfile
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg ci --amend --logfile ../logfile
  saved backup bundle to $TESTTMP/.hg/strip-backup/c16295aaf401-1ada9901-amend.hg (glob)

  $ hg cat b
  b: no such file in rev 47343646fa3d
  [1]

No changes, just a different message:

  $ hg ci -v --amend -m 'no changes, new message'
  amending changeset 47343646fa3d
  copying changeset 47343646fa3d to ad120869acf0
  committing files:
  a
  committing manifest
  committing changelog
  1 changesets found
  uncompressed size of bundle content:
       254 (changelog)
       163 (manifests)
       131  a
  saved backup bundle to $TESTTMP/.hg/strip-backup/47343646fa3d-c2758885-amend.hg (glob)
  1 changesets found
  uncompressed size of bundle content:
       250 (changelog)
       163 (manifests)
       131  a
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  committed changeset 1:401431e913a1
  $ hg diff -c .
  diff -r ad120869acf0 -r 401431e913a1 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,4 @@
   a
  +a
  +a
  +a
  $ hg log
  changeset:   1:401431e913a1
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

  $ cat > .hg/checkeditform.sh <<EOF
  > env | grep HGEDITFORM
  > true
  > EOF
  $ HGEDITOR="sh .hg/checkeditform.sh" hg ci --amend -u foo -d '1 0'
  HGEDITFORM=commit.amend.normal
  saved backup bundle to $TESTTMP/.hg/strip-backup/401431e913a1-5e8e532c-amend.hg (glob)
  $ echo a >> a
  $ hg ci --amend -u foo -d '1 0'
  saved backup bundle to $TESTTMP/.hg/strip-backup/d96b1d28ae33-677e0afb-amend.hg (glob)
  $ hg log -r .
  changeset:   1:a9a13940fc03
  tag:         tip
  user:        foo
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     no changes, new message
  

Open editor with old commit message if a message isn't given otherwise:

  $ cat > editor.sh << '__EOF__'
  > #!/bin/sh
  > cat $1
  > echo "another precious commit message" > "$1"
  > __EOF__

at first, test saving last-message.txt

  $ cat > .hg/hgrc << '__EOF__'
  > [hooks]
  > pretxncommit.test-saving-last-message = false
  > __EOF__

  $ rm -f .hg/last-message.txt
  $ hg commit --amend -v -m "message given from command line"
  amending changeset a9a13940fc03
  copying changeset a9a13940fc03 to ad120869acf0
  committing files:
  a
  committing manifest
  committing changelog
  running hook pretxncommit.test-saving-last-message: false
  transaction abort!
  rollback completed
  abort: pretxncommit.test-saving-last-message hook exited with status 1
  [255]
  $ cat .hg/last-message.txt
  message given from command line (no-eol)

  $ rm -f .hg/last-message.txt
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend -v
  amending changeset a9a13940fc03
  copying changeset a9a13940fc03 to ad120869acf0
  no changes, new message
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: foo
  HG: branch 'default'
  HG: changed a
  committing files:
  a
  committing manifest
  committing changelog
  running hook pretxncommit.test-saving-last-message: false
  transaction abort!
  rollback completed
  abort: pretxncommit.test-saving-last-message hook exited with status 1
  [255]

  $ cat .hg/last-message.txt
  another precious commit message

  $ cat > .hg/hgrc << '__EOF__'
  > [hooks]
  > pretxncommit.test-saving-last-message =
  > __EOF__

then, test editing custom commit message

  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend -v
  amending changeset a9a13940fc03
  copying changeset a9a13940fc03 to ad120869acf0
  no changes, new message
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: foo
  HG: branch 'default'
  HG: changed a
  committing files:
  a
  committing manifest
  committing changelog
  1 changesets found
  uncompressed size of bundle content:
       249 (changelog)
       163 (manifests)
       133  a
  saved backup bundle to $TESTTMP/.hg/strip-backup/a9a13940fc03-7c2e8674-amend.hg (glob)
  1 changesets found
  uncompressed size of bundle content:
       257 (changelog)
       163 (manifests)
       133  a
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  committed changeset 1:64a124ba1b44

Same, but with changes in working dir (different code path):

  $ echo a >> a
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend -v
  amending changeset 64a124ba1b44
  another precious commit message
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: foo
  HG: branch 'default'
  HG: changed a
  committing files:
  a
  committing manifest
  committing changelog
  1 changesets found
  uncompressed size of bundle content:
       257 (changelog)
       163 (manifests)
       133  a
  saved backup bundle to $TESTTMP/.hg/strip-backup/64a124ba1b44-10374b8f-amend.hg (glob)
  1 changesets found
  uncompressed size of bundle content:
       257 (changelog)
       163 (manifests)
       135  a
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  committed changeset 1:7892795b8e38

  $ rm editor.sh
  $ hg log -r .
  changeset:   1:7892795b8e38
  tag:         tip
  user:        foo
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     another precious commit message
  

Moving bookmarks, preserve active bookmark:

  $ hg book book1
  $ hg book book2
  $ hg ci --amend -m 'move bookmarks'
  saved backup bundle to $TESTTMP/.hg/strip-backup/7892795b8e38-3fb46217-amend.hg (glob)
  $ hg book
     book1                     1:8311f17e2616
   * book2                     1:8311f17e2616
  $ echo a >> a
  $ hg ci --amend -m 'move bookmarks'
  saved backup bundle to $TESTTMP/.hg/strip-backup/8311f17e2616-f0504fe3-amend.hg (glob)
  $ hg book
     book1                     1:a3b65065808c
   * book2                     1:a3b65065808c

abort does not loose bookmarks

  $ cat > editor.sh << '__EOF__'
  > #!/bin/sh
  > echo "" > "$1"
  > __EOF__
  $ echo a >> a
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend
  abort: empty commit message
  [255]
  $ hg book
     book1                     1:a3b65065808c
   * book2                     1:a3b65065808c
  $ hg revert -Caq
  $ rm editor.sh

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
  $ hg ci --amend -m 'back to default'
  saved backup bundle to $TESTTMP/.hg/strip-backup/f8339a38efe1-c18453c9-amend.hg (glob)
  $ hg branches
  default                        2:9c07515f2650

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
  saved backup bundle to $TESTTMP/.hg/strip-backup/c962248fa264-54245dc7-amend.hg (glob)

Same thing, different code path:

  $ echo b >> b
  $ hg ci -m 'reopen branch'
  reopening closed branch head 4
  $ echo b >> b
  $ hg ci --amend --close-branch
  saved backup bundle to $TESTTMP/.hg/strip-backup/027371728205-b900d9fa-amend.hg (glob)
  $ hg branches
  default                        2:9c07515f2650

Refuse to amend during a merge:

  $ hg up -q default
  $ hg merge foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci --amend
  abort: cannot amend while merging
  [255]
  $ hg ci -m 'merge'

Follow copies/renames:

  $ hg mv b c
  $ hg ci -m 'b -> c'
  $ hg mv c d
  $ hg ci --amend -m 'b -> d'
  saved backup bundle to $TESTTMP/.hg/strip-backup/42f3f27a067d-f23cc9f7-amend.hg (glob)
  $ hg st --rev '.^' --copies d
  A d
    b
  $ hg cp d e
  $ hg ci -m 'e = d'
  $ hg cp e f
  $ hg ci --amend -m 'f = d'
  saved backup bundle to $TESTTMP/.hg/strip-backup/9198f73182d5-251d584a-amend.hg (glob)
  $ hg st --rev '.^' --copies f
  A f
    d

  $ mv f f.orig
  $ hg rm -A f
  $ hg ci -m removef
  $ hg cp a f
  $ mv f.orig f
  $ hg ci --amend -m replacef
  saved backup bundle to $TESTTMP/.hg/strip-backup/f0993ab6b482-eda301bf-amend.hg (glob)
  $ hg st --change . --copies
  $ hg log -r . --template "{file_copies}\n"
  

Move added file (issue3410):

  $ echo g >> g
  $ hg ci -Am g
  adding g
  $ hg mv g h
  $ hg ci --amend
  saved backup bundle to $TESTTMP/.hg/strip-backup/58585e3f095c-0f5ebcda-amend.hg (glob)
  $ hg st --change . --copies h
  A h
  $ hg log -r . --template "{file_copies}\n"
  

Can't rollback an amend:

  $ hg rollback
  no rollback information available
  [1]

Preserve extra dict (issue3430):

  $ hg branch a
  marked working directory as branch a
  (branches are permanent and global, did you want a bookmark?)
  $ echo a >> a
  $ hg ci -ma
  $ hg ci --amend -m "a'"
  saved backup bundle to $TESTTMP/.hg/strip-backup/39a162f1d65e-9dfe13d8-amend.hg (glob)
  $ hg log -r . --template "{branch}\n"
  a
  $ hg ci --amend -m "a''"
  saved backup bundle to $TESTTMP/.hg/strip-backup/d5ca7b1ac72b-0b4c1a34-amend.hg (glob)
  $ hg log -r . --template "{branch}\n"
  a

Also preserve other entries in the dict that are in the old commit,
first graft something so there's an additional entry:

  $ hg up 0 -q
  $ echo z > z
  $ hg ci -Am 'fork'
  adding z
  created new head
  $ hg up 11
  5 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg graft 12
  grafting 12:2647734878ef "fork" (tip)
  $ hg ci --amend -m 'graft amend'
  saved backup bundle to $TESTTMP/.hg/strip-backup/fe8c6f7957ca-25638666-amend.hg (glob)
  $ hg log -r . --debug | grep extra
  extra:       amend_source=fe8c6f7957ca1665ed77496ed7a07657d469ac60
  extra:       branch=a
  extra:       source=2647734878ef0236dda712fae9c1651cf694ea8a

Preserve phase

  $ hg phase '.^::.'
  11: draft
  13: draft
  $ hg phase --secret --force .
  $ hg phase '.^::.'
  11: draft
  13: secret
  $ hg commit --amend -m 'amend for phase' -q
  $ hg phase '.^::.'
  11: draft
  13: secret

Test amend with obsolete
---------------------------

Enable obsolete

  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > evolution.createmarkers=True
  > evolution.allowunstable=True
  > EOF

Amend with no files changes

  $ hg id -n
  13
  $ hg ci --amend -m 'babar'
  $ hg id -n
  14
  $ hg log -Gl 3 --style=compact
  @  14[tip]:11   682950e85999   1970-01-01 00:00 +0000   test
  |    babar
  |
  | o  12:0   2647734878ef   1970-01-01 00:00 +0000   test
  | |    fork
  | ~
  o  11   0ddb275cfad1   1970-01-01 00:00 +0000   test
  |    a''
  ~
  $ hg log -Gl 4 --hidden --style=compact
  @  14[tip]:11   682950e85999   1970-01-01 00:00 +0000   test
  |    babar
  |
  | x  13:11   5167600b0f7a   1970-01-01 00:00 +0000   test
  |/     amend for phase
  |
  | o  12:0   2647734878ef   1970-01-01 00:00 +0000   test
  | |    fork
  | ~
  o  11   0ddb275cfad1   1970-01-01 00:00 +0000   test
  |    a''
  ~

Amend with files changes

(note: the extra commit over 15 is a temporary junk I would be happy to get
ride of)

  $ echo 'babar' >> a
  $ hg commit --amend
  $ hg log -Gl 6 --hidden --style=compact
  @  15[tip]:11   a5b42b49b0d5   1970-01-01 00:00 +0000   test
  |    babar
  |
  | x  14:11   682950e85999   1970-01-01 00:00 +0000   test
  |/     babar
  |
  | x  13:11   5167600b0f7a   1970-01-01 00:00 +0000   test
  |/     amend for phase
  |
  | o  12:0   2647734878ef   1970-01-01 00:00 +0000   test
  | |    fork
  | ~
  o  11   0ddb275cfad1   1970-01-01 00:00 +0000   test
  |    a''
  |
  o  10   5fa75032e226   1970-01-01 00:00 +0000   test
  |    g
  ~


Test that amend does not make it easy to create obsolescence cycle
---------------------------------------------------------------------

  $ hg id -r 14 --hidden
  682950e85999 (a)
  $ hg revert -ar 14 --hidden
  reverting a
  $ hg commit --amend
  $ hg id
  37973c7e0b61 (a) tip

Test that rewriting leaving instability behind is allowed
---------------------------------------------------------------------

  $ hg up '.^'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'b' >> a
  $ hg log --style compact -r 'children(.)'
  16[tip]:11   37973c7e0b61   1970-01-01 00:00 +0000   test
    babar
  
  $ hg commit --amend
  $ hg log -r 'orphan()'
  changeset:   16:37973c7e0b61
  branch:      a
  parent:      11:0ddb275cfad1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  instability: orphan
  summary:     babar
  

Amend a merge changeset (with renames and conflicts from the second parent):

  $ hg up -q default
  $ hg branch -q bar
  $ hg cp a aa
  $ hg mv z zz
  $ echo cc > cc
  $ hg add cc
  $ hg ci -m aazzcc
  $ hg up -q default
  $ echo a >> a
  $ echo dd > cc
  $ hg add cc
  $ hg ci -m aa
  $ hg merge -q bar
  warning: conflicts while merging cc! (edit, then use 'hg resolve --mark')
  [1]
  $ hg resolve -m cc
  (no more unresolved files)
  $ hg ci -m 'merge bar'
  $ hg log --config diff.git=1 -pr .
  changeset:   20:163cfd7219f7
  tag:         tip
  parent:      19:30d96aeaf27b
  parent:      18:1aa437659d19
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge bar
  
  diff --git a/a b/aa
  copy from a
  copy to aa
  diff --git a/cc b/cc
  --- a/cc
  +++ b/cc
  @@ -1,1 +1,5 @@
  +<<<<<<< working copy: 30d96aeaf27b - test: aa
   dd
  +=======
  +cc
  +>>>>>>> merge rev:    1aa437659d19 bar - test: aazzcc
  diff --git a/z b/zz
  rename from z
  rename to zz
  
  $ hg debugrename aa
  aa renamed from a:a80d06849b333b8a3d5c445f8ba3142010dcdc9e
  $ hg debugrename zz
  zz renamed from z:69a1b67522704ec122181c0890bd16e9d3e7516a
  $ hg debugrename cc
  cc not renamed
  $ HGEDITOR="sh .hg/checkeditform.sh" hg ci --amend -m 'merge bar (amend message)' --edit
  HGEDITFORM=commit.amend.merge
  $ hg log --config diff.git=1 -pr .
  changeset:   21:bca52d4ed186
  tag:         tip
  parent:      19:30d96aeaf27b
  parent:      18:1aa437659d19
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge bar (amend message)
  
  diff --git a/a b/aa
  copy from a
  copy to aa
  diff --git a/cc b/cc
  --- a/cc
  +++ b/cc
  @@ -1,1 +1,5 @@
  +<<<<<<< working copy: 30d96aeaf27b - test: aa
   dd
  +=======
  +cc
  +>>>>>>> merge rev:    1aa437659d19 bar - test: aazzcc
  diff --git a/z b/zz
  rename from z
  rename to zz
  
  $ hg debugrename aa
  aa renamed from a:a80d06849b333b8a3d5c445f8ba3142010dcdc9e
  $ hg debugrename zz
  zz renamed from z:69a1b67522704ec122181c0890bd16e9d3e7516a
  $ hg debugrename cc
  cc not renamed
  $ hg mv zz z
  $ hg ci --amend -m 'merge bar (undo rename)'
  $ hg log --config diff.git=1 -pr .
  changeset:   22:12594a98ca3f
  tag:         tip
  parent:      19:30d96aeaf27b
  parent:      18:1aa437659d19
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge bar (undo rename)
  
  diff --git a/a b/aa
  copy from a
  copy to aa
  diff --git a/cc b/cc
  --- a/cc
  +++ b/cc
  @@ -1,1 +1,5 @@
  +<<<<<<< working copy: 30d96aeaf27b - test: aa
   dd
  +=======
  +cc
  +>>>>>>> merge rev:    1aa437659d19 bar - test: aazzcc
  
  $ hg debugrename z
  z not renamed

Amend a merge changeset (with renames during the merge):

  $ hg up -q bar
  $ echo x > x
  $ hg add x
  $ hg ci -m x
  $ hg up -q default
  $ hg merge -q bar
  $ hg mv aa aaa
  $ echo aa >> aaa
  $ hg ci -m 'merge bar again'
  $ hg log --config diff.git=1 -pr .
  changeset:   24:dffde028b388
  tag:         tip
  parent:      22:12594a98ca3f
  parent:      23:4c94d5bc65f5
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge bar again
  
  diff --git a/aa b/aa
  deleted file mode 100644
  --- a/aa
  +++ /dev/null
  @@ -1,2 +0,0 @@
  -a
  -a
  diff --git a/aaa b/aaa
  new file mode 100644
  --- /dev/null
  +++ b/aaa
  @@ -0,0 +1,3 @@
  +a
  +a
  +aa
  diff --git a/x b/x
  new file mode 100644
  --- /dev/null
  +++ b/x
  @@ -0,0 +1,1 @@
  +x
  
  $ hg debugrename aaa
  aaa renamed from aa:37d9b5d994eab34eda9c16b195ace52c7b129980
  $ hg mv aaa aa
  $ hg ci --amend -m 'merge bar again (undo rename)'
  $ hg log --config diff.git=1 -pr .
  changeset:   25:18e3ba160489
  tag:         tip
  parent:      22:12594a98ca3f
  parent:      23:4c94d5bc65f5
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge bar again (undo rename)
  
  diff --git a/aa b/aa
  --- a/aa
  +++ b/aa
  @@ -1,2 +1,3 @@
   a
   a
  +aa
  diff --git a/x b/x
  new file mode 100644
  --- /dev/null
  +++ b/x
  @@ -0,0 +1,1 @@
  +x
  
  $ hg debugrename aa
  aa not renamed
  $ hg debugrename -r '.^' aa
  aa renamed from a:a80d06849b333b8a3d5c445f8ba3142010dcdc9e

Amend a merge changeset (with manifest-level conflicts):

  $ hg up -q bar
  $ hg rm aa
  $ hg ci -m 'rm aa'
  $ hg up -q default
  $ echo aa >> aa
  $ hg ci -m aa
  $ hg merge -q bar --config ui.interactive=True << EOF
  > c
  > EOF
  local [working copy] changed aa which other [merge rev] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? c
  $ hg ci -m 'merge bar (with conflicts)'
  $ hg log --config diff.git=1 -pr .
  changeset:   28:b4c3035e2544
  tag:         tip
  parent:      27:4b216ca5ba97
  parent:      26:67db8847a540
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge bar (with conflicts)
  
  
  $ hg rm aa
  $ hg ci --amend -m 'merge bar (with conflicts, amended)'
  $ hg log --config diff.git=1 -pr .
  changeset:   29:1205ed810051
  tag:         tip
  parent:      27:4b216ca5ba97
  parent:      26:67db8847a540
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge bar (with conflicts, amended)
  
  diff --git a/aa b/aa
  deleted file mode 100644
  --- a/aa
  +++ /dev/null
  @@ -1,4 +0,0 @@
  -a
  -a
  -aa
  -aa
  
Issue 3445: amending with --close-branch a commit that created a new head should fail
This shouldn't be possible:

  $ hg up -q default
  $ hg branch closewithamend
  marked working directory as branch closewithamend
  $ echo foo > foo
  $ hg add foo
  $ hg ci -m..
  $ hg ci --amend --close-branch -m 'closing'
  abort: can only close branch heads
  [255]

This silliness fails:

  $ hg branch silliness
  marked working directory as branch silliness
  $ echo b >> b
  $ hg ci --close-branch -m'open and close'
  abort: can only close branch heads
  [255]

Test that amend with --secret creates new secret changeset forcibly
---------------------------------------------------------------------

  $ hg phase '.^::.'
  29: draft
  30: draft
  $ hg commit --amend --secret -m 'amend as secret' -q
  $ hg phase '.^::.'
  29: draft
  31: secret

Test that amend with --edit invokes editor forcibly
---------------------------------------------------

  $ hg parents --template "{desc}\n"
  amend as secret
  $ HGEDITOR=cat hg commit --amend -m "editor should be suppressed"
  $ hg parents --template "{desc}\n"
  editor should be suppressed

  $ hg status --rev '.^1::.'
  A foo
  $ HGEDITOR=cat hg commit --amend -m "editor should be invoked" --edit
  editor should be invoked
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'silliness'
  HG: added foo
  $ hg parents --template "{desc}\n"
  editor should be invoked

Test that "diff()" in committemplate works correctly for amending
-----------------------------------------------------------------

  $ cat >> .hg/hgrc <<EOF
  > [committemplate]
  > changeset.commit.amend = {desc}\n
  >     HG: M: {file_mods}
  >     HG: A: {file_adds}
  >     HG: R: {file_dels}
  >     {splitlines(diff()) % 'HG: {line}\n'}
  > EOF

  $ hg parents --template "M: {file_mods}\nA: {file_adds}\nR: {file_dels}\n"
  M: 
  A: foo
  R: 
  $ hg status -amr
  $ HGEDITOR=cat hg commit --amend -e -m "expecting diff of foo"
  expecting diff of foo
  
  HG: M: 
  HG: A: foo
  HG: R: 
  HG: diff -r 1205ed810051 foo
  HG: --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -0,0 +1,1 @@
  HG: +foo

  $ echo y > y
  $ hg add y
  $ HGEDITOR=cat hg commit --amend -e -m "expecting diff of foo and y"
  expecting diff of foo and y
  
  HG: M: 
  HG: A: foo y
  HG: R: 
  HG: diff -r 1205ed810051 foo
  HG: --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -0,0 +1,1 @@
  HG: +foo
  HG: diff -r 1205ed810051 y
  HG: --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ b/y	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -0,0 +1,1 @@
  HG: +y

  $ hg rm a
  $ HGEDITOR=cat hg commit --amend -e -m "expecting diff of a, foo and y"
  expecting diff of a, foo and y
  
  HG: M: 
  HG: A: foo y
  HG: R: a
  HG: diff -r 1205ed810051 a
  HG: --- a/a	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -1,2 +0,0 @@
  HG: -a
  HG: -a
  HG: diff -r 1205ed810051 foo
  HG: --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -0,0 +1,1 @@
  HG: +foo
  HG: diff -r 1205ed810051 y
  HG: --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ b/y	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -0,0 +1,1 @@
  HG: +y

  $ hg rm x
  $ HGEDITOR=cat hg commit --amend -e -m "expecting diff of a, foo, x and y"
  expecting diff of a, foo, x and y
  
  HG: M: 
  HG: A: foo y
  HG: R: a x
  HG: diff -r 1205ed810051 a
  HG: --- a/a	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -1,2 +0,0 @@
  HG: -a
  HG: -a
  HG: diff -r 1205ed810051 foo
  HG: --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -0,0 +1,1 @@
  HG: +foo
  HG: diff -r 1205ed810051 x
  HG: --- a/x	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -1,1 +0,0 @@
  HG: -x
  HG: diff -r 1205ed810051 y
  HG: --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ b/y	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -0,0 +1,1 @@
  HG: +y

  $ echo cccc >> cc
  $ hg status -amr
  M cc
  $ HGEDITOR=cat hg commit --amend -e -m "cc should be excluded" -X cc
  cc should be excluded
  
  HG: M: 
  HG: A: foo y
  HG: R: a x
  HG: diff -r 1205ed810051 a
  HG: --- a/a	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -1,2 +0,0 @@
  HG: -a
  HG: -a
  HG: diff -r 1205ed810051 foo
  HG: --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ b/foo	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -0,0 +1,1 @@
  HG: +foo
  HG: diff -r 1205ed810051 x
  HG: --- a/x	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -1,1 +0,0 @@
  HG: -x
  HG: diff -r 1205ed810051 y
  HG: --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ b/y	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -0,0 +1,1 @@
  HG: +y

Check for issue4405
-------------------

Setup the repo with a file that gets moved in a second commit.
  $ hg init repo
  $ cd repo
  $ touch a0
  $ hg add a0
  $ hg commit -m a0
  $ hg mv a0 a1
  $ hg commit -m a1
  $ hg up -q 0
  $ hg log -G --template '{rev} {desc}'
  o  1 a1
  |
  @  0 a0
  

Now we branch the repro, but re-use the file contents, so we have a divergence
in the file revlog topology and the changelog topology.
  $ hg revert --rev 1 --all
  removing a0
  adding a1
  $ hg ci -qm 'a1-amend'
  $ hg log -G --template '{rev} {desc}'
  @  2 a1-amend
  |
  | o  1 a1
  |/
  o  0 a0
  

The way mercurial does amends is by folding the working copy and old commit
together into another commit (rev 3). During this process, _findlimit is called
to  check how far back to look for the transitive closure of file copy
information, but due to the divergence of the filelog and changelog graph
topologies, before _findlimit was fixed, it returned a rev which was not far
enough back in this case.
  $ hg mv a1 a2
  $ hg status --copies --rev 0
  A a2
    a0
  R a0
  $ hg ci --amend -q
  $ hg log -G --template '{rev} {desc}'
  @  3 a1-amend
  |
  | o  1 a1
  |/
  o  0 a0
  

Before the fix, the copy information was lost.
  $ hg status --copies --rev 0
  A a2
    a0
  R a0
  $ cd ..

Check that amend properly preserve rename from directory rename (issue-4516)

If a parent of the merge renames a full directory, any files added to the old
directory in the other parent will be renamed to the new directory. For some
reason, the rename metadata was when amending such merge. This test ensure we
do not regress. We have a dedicated repo because it needs a setup with renamed
directory)

  $ hg init issue4516
  $ cd issue4516
  $ mkdir olddirname
  $ echo line1 > olddirname/commonfile.py
  $ hg add olddirname/commonfile.py
  $ hg ci -m first

  $ hg branch newdirname
  marked working directory as branch newdirname
  (branches are permanent and global, did you want a bookmark?)
  $ hg mv olddirname newdirname
  moving olddirname/commonfile.py to newdirname/commonfile.py (glob)
  $ hg ci -m rename

  $ hg update default
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo line1 > olddirname/newfile.py
  $ hg add olddirname/newfile.py
  $ hg ci -m log

  $ hg up newdirname
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ # create newdirname/newfile.py
  $ hg merge default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m add
  $ 
  $ hg debugrename newdirname/newfile.py
  newdirname/newfile.py renamed from olddirname/newfile.py:690b295714aed510803d3020da9c70fca8336def (glob)
  $ hg status -C --change .
  A newdirname/newfile.py
  $ hg status -C --rev 1
  A newdirname/newfile.py
  $ hg status -C --rev 2
  A newdirname/commonfile.py
    olddirname/commonfile.py
  A newdirname/newfile.py
    olddirname/newfile.py
  R olddirname/commonfile.py
  R olddirname/newfile.py
  $ hg debugindex newdirname/newfile.py
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      89     -1       3 34a4d536c0c0 000000000000 000000000000

  $ echo a >> newdirname/commonfile.py
  $ hg ci --amend -m bug
  $ hg debugrename newdirname/newfile.py
  newdirname/newfile.py renamed from olddirname/newfile.py:690b295714aed510803d3020da9c70fca8336def (glob)
  $ hg debugindex newdirname/newfile.py
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      89     -1       3 34a4d536c0c0 000000000000 000000000000

#if execbit

Test if amend preserves executable bit changes
  $ chmod +x newdirname/commonfile.py
  $ hg ci -m chmod
  $ hg ci --amend -m "chmod amended"
  $ hg ci --amend -m "chmod amended second time"
  $ hg log -p --git -r .
  changeset:   7:b1326f52dddf
  branch:      newdirname
  tag:         tip
  parent:      4:7fd235f7cb2f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     chmod amended second time
  
  diff --git a/newdirname/commonfile.py b/newdirname/commonfile.py
  old mode 100644
  new mode 100755
  
#endif

Test amend with file inclusion options
--------------------------------------

These tests ensure that we are always amending some files that were part of the
pre-amend commit. We want to test that the remaining files in the pre-amend
commit were not changed in the amended commit. We do so by performing a diff of
the amended commit against its parent commit.
  $ cd ..
  $ hg init testfileinclusions
  $ cd testfileinclusions
  $ echo a > a
  $ echo b > b
  $ hg commit -Aqm "Adding a and b"

Only add changes to a particular file
  $ echo a >> a
  $ echo b >> b
  $ hg commit --amend -I a
  $ hg diff --git -r null -r .
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,2 @@
  +a
  +a
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +b

  $ echo a >> a
  $ hg commit --amend b
  $ hg diff --git -r null -r .
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,2 @@
  +a
  +a
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,2 @@
  +b
  +b

Exclude changes to a particular file
  $ echo b >> b
  $ hg commit --amend -X a
  $ hg diff --git -r null -r .
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,2 @@
  +a
  +a
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,3 @@
  +b
  +b
  +b

Check the addremove flag
  $ echo c > c
  $ rm a
  $ hg commit --amend -A
  removing a
  adding c
  $ hg diff --git -r null -r .
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,3 @@
  +b
  +b
  +b
  diff --git a/c b/c
  new file mode 100644
  --- /dev/null
  +++ b/c
  @@ -0,0 +1,1 @@
  +c

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
  saved backup bundle to $TESTTMP/.hg/strip-backup/489edb5b847d-amend-backup.hg (glob)
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
  $ echo b > b
  $ hg add b
  $ hg summary
  parent: 1:43f1ba15f28a tip
   amend base1
  branch: default
  commit: 1 added, 1 unknown
  update: (current)
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend
  transaction abort!
  rollback completed
  abort: empty commit message
  [255]
  $ hg summary
  parent: 1:43f1ba15f28a tip
   amend base1
  branch: default
  commit: 1 added, 1 unknown
  update: (current)

Add new file:
  $ hg ci --amend -m 'amend base1 new file'
  saved backup bundle to $TESTTMP/.hg/strip-backup/43f1ba15f28a-amend-backup.hg (glob)

Remove file that was added in amended commit:
(and test logfile option)
(and test that logfile option do not trigger an editor)

  $ hg rm b
  $ echo 'amend base1 remove new file' > ../logfile
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg ci --amend --logfile ../logfile
  saved backup bundle to $TESTTMP/.hg/strip-backup/b8e3cb2b3882-amend-backup.hg (glob)

  $ hg cat b
  b: no such file in rev 74609c7f506e
  [1]

No changes, just a different message:

  $ hg ci -v --amend -m 'no changes, new message'
  amending changeset 74609c7f506e
  copying changeset 74609c7f506e to ad120869acf0
  a
  stripping amended changeset 74609c7f506e
  1 changesets found
  saved backup bundle to $TESTTMP/.hg/strip-backup/74609c7f506e-amend-backup.hg (glob)
  1 changesets found
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  committed changeset 1:1cd866679df8
  $ hg diff -c .
  diff -r ad120869acf0 -r 1cd866679df8 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,3 @@
   a
  +a
  +a
  $ hg log
  changeset:   1:1cd866679df8
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
  saved backup bundle to $TESTTMP/.hg/strip-backup/1cd866679df8-amend-backup.hg (glob)
  $ echo a >> a
  $ hg ci --amend -u foo -d '1 0'
  saved backup bundle to $TESTTMP/.hg/strip-backup/780e6f23e03d-amend-backup.hg (glob)
  $ hg log -r .
  changeset:   1:5f357c7560ab
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
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend -v
  amending changeset 5f357c7560ab
  copying changeset 5f357c7560ab to ad120869acf0
  no changes, new message
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: foo
  HG: branch 'default'
  HG: changed a
  a
  stripping amended changeset 5f357c7560ab
  1 changesets found
  saved backup bundle to $TESTTMP/.hg/strip-backup/5f357c7560ab-amend-backup.hg (glob)
  1 changesets found
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  committed changeset 1:7ab3bf440b54

Same, but with changes in working dir (different code path):

  $ echo a >> a
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend -v
  amending changeset 7ab3bf440b54
  a
  copying changeset a0ea9b1a4c8c to ad120869acf0
  another precious commit message
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: foo
  HG: branch 'default'
  HG: changed a
  a
  stripping intermediate changeset a0ea9b1a4c8c
  stripping amended changeset 7ab3bf440b54
  2 changesets found
  saved backup bundle to $TESTTMP/.hg/strip-backup/7ab3bf440b54-amend-backup.hg (glob)
  1 changesets found
  adding branch
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  committed changeset 1:ea22a388757c

  $ rm editor.sh
  $ hg log -r .
  changeset:   1:ea22a388757c
  tag:         tip
  user:        foo
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     another precious commit message
  

Moving bookmarks, preserve active bookmark:

  $ hg book book1
  $ hg book book2
  $ hg ci --amend -m 'move bookmarks'
  saved backup bundle to $TESTTMP/.hg/strip-backup/ea22a388757c-amend-backup.hg (glob)
  $ hg book
     book1                     1:6cec5aa930e2
   * book2                     1:6cec5aa930e2
  $ echo a >> a
  $ hg ci --amend -m 'move bookmarks'
  saved backup bundle to $TESTTMP/.hg/strip-backup/6cec5aa930e2-amend-backup.hg (glob)
  $ hg book
     book1                     1:48bb6e53a15f
   * book2                     1:48bb6e53a15f

abort does not loose bookmarks

  $ cat > editor.sh << '__EOF__'
  > #!/bin/sh
  > echo "" > "$1"
  > __EOF__
  $ echo a >> a
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend
  transaction abort!
  rollback completed
  abort: empty commit message
  [255]
  $ hg book
     book1                     1:48bb6e53a15f
   * book2                     1:48bb6e53a15f
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
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci --amend -m 'back to default'
  saved backup bundle to $TESTTMP/.hg/strip-backup/8ac881fbf49d-amend-backup.hg (glob)
  $ hg branches
  default                        2:ce12b0b57d46

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
  saved backup bundle to $TESTTMP/.hg/strip-backup/c962248fa264-amend-backup.hg (glob)

Same thing, different code path:

  $ echo b >> b
  $ hg ci -m 'reopen branch'
  reopening closed branch head 4
  $ echo b >> b
  $ hg ci --amend --close-branch
  saved backup bundle to $TESTTMP/.hg/strip-backup/027371728205-amend-backup.hg (glob)
  $ hg branches
  default                        2:ce12b0b57d46

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
  saved backup bundle to $TESTTMP/.hg/strip-backup/b8c6eac7f12e-amend-backup.hg (glob)
  $ hg st --rev '.^' --copies d
  A d
    b
  $ hg cp d e
  $ hg ci -m 'e = d'
  $ hg cp e f
  $ hg ci --amend -m 'f = d'
  saved backup bundle to $TESTTMP/.hg/strip-backup/7f9761d65613-amend-backup.hg (glob)
  $ hg st --rev '.^' --copies f
  A f
    d

  $ mv f f.orig
  $ hg rm -A f
  $ hg ci -m removef
  $ hg cp a f
  $ mv f.orig f
  $ hg ci --amend -m replacef
  saved backup bundle to $TESTTMP/.hg/strip-backup/9e8c5f7e3d95-amend-backup.hg (glob)
  $ hg st --change . --copies
  $ hg log -r . --template "{file_copies}\n"
  

Move added file (issue3410):

  $ echo g >> g
  $ hg ci -Am g
  adding g
  $ hg mv g h
  $ hg ci --amend
  saved backup bundle to $TESTTMP/.hg/strip-backup/24aa8eacce2b-amend-backup.hg (glob)
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
  saved backup bundle to $TESTTMP/.hg/strip-backup/3837aa2a2fdb-amend-backup.hg (glob)
  $ hg log -r . --template "{branch}\n"
  a
  $ hg ci --amend -m "a''"
  saved backup bundle to $TESTTMP/.hg/strip-backup/c05c06be7514-amend-backup.hg (glob)
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
  grafting revision 12
  $ hg ci --amend -m 'graft amend'
  saved backup bundle to $TESTTMP/.hg/strip-backup/bd010aea3f39-amend-backup.hg (glob)
  $ hg log -r . --debug | grep extra
  extra:       amend_source=bd010aea3f39f3fb2a2f884b9ccb0471cd77398e
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

  $ cat > ${TESTTMP}/obs.py << EOF
  > import mercurial.obsolete
  > mercurial.obsolete._enabled = True
  > EOF
  $ echo '[extensions]' >> $HGRCPATH
  $ echo "obs=${TESTTMP}/obs.py" >> $HGRCPATH


Amend with no files changes

  $ hg id -n
  13
  $ hg ci --amend -m 'babar'
  $ hg id -n
  14
  $ hg log -Gl 3 --style=compact
  @  14[tip]:11   b650e6ee8614   1970-01-01 00:00 +0000   test
  |    babar
  |
  | o  12:0   2647734878ef   1970-01-01 00:00 +0000   test
  | |    fork
  | |
  o |  11   3334b7925910   1970-01-01 00:00 +0000   test
  | |    a''
  | |
  $ hg log -Gl 4 --hidden --style=compact
  @  14[tip]:11   b650e6ee8614   1970-01-01 00:00 +0000   test
  |    babar
  |
  | x  13:11   68ff8ff97044   1970-01-01 00:00 +0000   test
  |/     amend for phase
  |
  | o  12:0   2647734878ef   1970-01-01 00:00 +0000   test
  | |    fork
  | |
  o |  11   3334b7925910   1970-01-01 00:00 +0000   test
  | |    a''
  | |

Amend with files changes

(note: the extra commit over 15 is a temporary junk I would be happy to get
ride of)

  $ echo 'babar' >> a
  $ hg commit --amend
  $ hg log -Gl 6 --hidden --style=compact
  @  16[tip]:11   9f9e9bccf56c   1970-01-01 00:00 +0000   test
  |    babar
  |
  | x  15   90fef497c56f   1970-01-01 00:00 +0000   test
  | |    temporary amend commit for b650e6ee8614
  | |
  | x  14:11   b650e6ee8614   1970-01-01 00:00 +0000   test
  |/     babar
  |
  | x  13:11   68ff8ff97044   1970-01-01 00:00 +0000   test
  |/     amend for phase
  |
  | o  12:0   2647734878ef   1970-01-01 00:00 +0000   test
  | |    fork
  | |
  o |  11   3334b7925910   1970-01-01 00:00 +0000   test
  | |    a''
  | |


Test that amend does not make it easy to create obsolescence cycle
---------------------------------------------------------------------

  $ hg id -r 14 --hidden
  b650e6ee8614 (a)
  $ hg revert -ar 14 --hidden
  reverting a
  $ hg commit --amend
  $ hg id
  b99e5df575f7 (a) tip

Test that rewriting leaving instability behind is allowed
---------------------------------------------------------------------

  $ hg up '.^'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 'b' >> a
  $ hg log --style compact -r 'children(.)'
  18[tip]:11   b99e5df575f7   1970-01-01 00:00 +0000   test
    babar
  
  $ hg commit --amend
  $ hg log -r 'unstable()'
  changeset:   18:b99e5df575f7
  branch:      a
  parent:      11:3334b7925910
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
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
  warning: conflicts during merge.
  merging cc incomplete! (edit conflicts, then use 'hg resolve --mark')
  [1]
  $ hg resolve -m cc
  $ hg ci -m 'merge bar'
  $ hg log --config diff.git=1 -pr .
  changeset:   23:d51446492733
  tag:         tip
  parent:      22:30d96aeaf27b
  parent:      21:1aa437659d19
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
  +<<<<<<< local
   dd
  +=======
  +cc
  +>>>>>>> other
  diff --git a/z b/zz
  rename from z
  rename to zz
  
  $ hg debugrename aa
  aa renamed from a:a80d06849b333b8a3d5c445f8ba3142010dcdc9e
  $ hg debugrename zz
  zz renamed from z:69a1b67522704ec122181c0890bd16e9d3e7516a
  $ hg debugrename cc
  cc not renamed
  $ hg ci --amend -m 'merge bar (amend message)'
  $ hg log --config diff.git=1 -pr .
  changeset:   24:59de3dce7a79
  tag:         tip
  parent:      22:30d96aeaf27b
  parent:      21:1aa437659d19
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
  +<<<<<<< local
   dd
  +=======
  +cc
  +>>>>>>> other
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
  changeset:   26:7fb89c461f81
  tag:         tip
  parent:      22:30d96aeaf27b
  parent:      21:1aa437659d19
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
  +<<<<<<< local
   dd
  +=======
  +cc
  +>>>>>>> other
  
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
  changeset:   28:982d7a34ffee
  tag:         tip
  parent:      26:7fb89c461f81
  parent:      27:4c94d5bc65f5
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
  changeset:   30:522688c0e71b
  tag:         tip
  parent:      26:7fb89c461f81
  parent:      27:4c94d5bc65f5
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
  $ hg merge -q bar
  local changed aa which remote deleted
  use (c)hanged version or (d)elete? c
  $ hg ci -m 'merge bar (with conflicts)'
  $ hg log --config diff.git=1 -pr .
  changeset:   33:5f9904c491b8
  tag:         tip
  parent:      32:01780b896f58
  parent:      31:67db8847a540
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge bar (with conflicts)
  
  
  $ hg rm aa
  $ hg ci --amend -m 'merge bar (with conflicts, amended)'
  $ hg log --config diff.git=1 -pr .
  changeset:   35:6ce0c89781a3
  tag:         tip
  parent:      32:01780b896f58
  parent:      31:67db8847a540
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
  (branches are permanent and global, did you want a bookmark?)
  $ hg add obs.py
  $ hg ci -m..
  $ hg ci --amend --close-branch -m 'closing'
  abort: can only close branch heads
  [255]

This silliness fails:

  $ hg branch silliness
  marked working directory as branch silliness
  (branches are permanent and global, did you want a bookmark?)
  $ echo b >> b
  $ hg ci --close-branch -m'open and close'
  abort: can only close branch heads
  [255]

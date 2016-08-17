Test uncommit - set up the config

  $ cat >> $HGRCPATH <<EOF
  > [experimental]
  > evolution=createmarkers
  > [extensions]
  > uncommit = $TESTDIR/../hgext3rd/uncommit.py
  > EOF

Build up a repo

  $ hg init repo
  $ cd repo

Uncommit with no commits should fail

  $ hg uncommit
  abort: cannot uncommit null changeset
  [255]

Create some commits

  $ touch files
  $ hg add files
  $ for i in a ab abc abcd abcde; do echo $i > files; echo $i > file-$i; hg add file-$i; hg commit -m "added file-$i"; done
  $ ls
  file-a
  file-ab
  file-abc
  file-abcd
  file-abcde
  files
  $ hg log -G -T '{rev}:{node} {desc}' --hidden
  @  4:6c4fd43ed714e7fcd8adbaa7b16c953c2e985b60 added file-abcde
  |
  o  3:6db330d65db434145c0b59d291853e9a84719b24 added file-abcd
  |
  o  2:abf2df566fc193b3ac34d946e63c1583e4d4732b added file-abc
  |
  o  1:69a232e754b08d568c4899475faf2eb44b857802 added file-ab
  |
  o  0:3004d2d9b50883c1538fc754a3aeb55f1b4084f6 added file-a
  
Simple uncommit off the top

  $ hg uncommit
  $ hg status
  M files
  A file-abcde
  $ hg heads -T '{rev}:{node} {desc}'
  3:6db330d65db434145c0b59d291853e9a84719b24 added file-abcd (no-eol)

Recommit

  $ hg commit -m 'new change abcde'
  $ hg status
  $ hg heads -T '{rev}:{node} {desc}'
  5:0c07a3ccda771b25f1cb1edbd02e683723344ef1 new change abcde (no-eol)

Uncommit of non-existent and unchanged files has no effect
  $ hg uncommit nothinghere
  abort: nothing to uncommit
  [255]
  $ hg status
  $ hg uncommit file-abc
  abort: nothing to uncommit
  [255]
  $ hg status

Try partial uncommit

  $ hg uncommit files
  $ hg status
  M files
  $ hg heads -T '{rev}:{node} {desc}'
  6:3727deee06f72f5ffa8db792ee299cf39e3e190b new change abcde (no-eol)
  $ hg log -r . -p -T '{rev}:{node} {desc}'
  6:3727deee06f72f5ffa8db792ee299cf39e3e190b new change abcdediff -r 6db330d65db4 -r 3727deee06f7 file-abcde
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/file-abcde	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +abcde
  
  $ hg commit -m 'update files for abcde'

Uncommit with dirty state

  $ echo "foo" >> files
  $ cat files
  abcde
  foo
  $ hg status
  M files
  $ hg uncommit files
  $ cat files
  abcde
  foo
  $ hg commit -m "files abcde + foo"

Uncommit in the middle of a stack

  $ hg checkout '.^^^'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -r . -p -T '{rev}:{node} {desc}'
  2:abf2df566fc193b3ac34d946e63c1583e4d4732b added file-abcdiff -r 69a232e754b0 -r abf2df566fc1 file-abc
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/file-abc	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +abc
  diff -r 69a232e754b0 -r abf2df566fc1 files
  --- a/files	Thu Jan 01 00:00:00 1970 +0000
  +++ b/files	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -ab
  +abc
  
  $ hg uncommit
  $ hg status
  M files
  A file-abc
  $ hg heads -T '{rev}:{node} {desc}'
  8:83815831694b1271e9f207cb1b79b2b19275edcb files abcde + foo (no-eol)
  $ hg commit -m 'new abc'
  created new head

Partial uncommit in the middle

  $ hg checkout '.^'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg log -r . -p -T '{rev}:{node} {desc}'
  1:69a232e754b08d568c4899475faf2eb44b857802 added file-abdiff -r 3004d2d9b508 -r 69a232e754b0 file-ab
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/file-ab	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +ab
  diff -r 3004d2d9b508 -r 69a232e754b0 files
  --- a/files	Thu Jan 01 00:00:00 1970 +0000
  +++ b/files	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -a
  +ab
  
  $ hg uncommit file-ab
  $ hg status
  A file-ab
  $ hg heads -T '{rev}:{node} {desc}'
  10:8eb87968f2edb7f27f27fe676316e179de65fff6 added file-ab9:5dc89ca4486f8a88716c5797fa9f498d13d7c2e1 new abc8:83815831694b1271e9f207cb1b79b2b19275edcb files abcde + foo (no-eol)
  $ hg commit -m 'update ab'
  $ hg status
  $ hg heads -T '{rev}:{node} {desc}'
  11:f21039c59242b085491bb58f591afc4ed1c04c09 update ab9:5dc89ca4486f8a88716c5797fa9f498d13d7c2e1 new abc8:83815831694b1271e9f207cb1b79b2b19275edcb files abcde + foo (no-eol)
  $ hg log -G -T '{rev}:{node} {desc}' --hidden
  @  11:f21039c59242b085491bb58f591afc4ed1c04c09 update ab
  |
  o  10:8eb87968f2edb7f27f27fe676316e179de65fff6 added file-ab
  |
  | o  9:5dc89ca4486f8a88716c5797fa9f498d13d7c2e1 new abc
  | |
  | | o  8:83815831694b1271e9f207cb1b79b2b19275edcb files abcde + foo
  | | |
  | | | x  7:0977fa602c2fd7d8427ed4e7ee15ea13b84c9173 update files for abcde
  | | |/
  | | o  6:3727deee06f72f5ffa8db792ee299cf39e3e190b new change abcde
  | | |
  | | | x  5:0c07a3ccda771b25f1cb1edbd02e683723344ef1 new change abcde
  | | |/
  | | | x  4:6c4fd43ed714e7fcd8adbaa7b16c953c2e985b60 added file-abcde
  | | |/
  | | o  3:6db330d65db434145c0b59d291853e9a84719b24 added file-abcd
  | | |
  | | x  2:abf2df566fc193b3ac34d946e63c1583e4d4732b added file-abc
  | |/
  | x  1:69a232e754b08d568c4899475faf2eb44b857802 added file-ab
  |/
  o  0:3004d2d9b50883c1538fc754a3aeb55f1b4084f6 added file-a
  


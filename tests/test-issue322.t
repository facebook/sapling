http://mercurial.selenic.com/bts/issue322

File replaced with directory:

  $ hg init a
  $ cd a
  $ echo a > a
  $ hg commit -Ama
  adding a
  $ rm a
  $ mkdir a
  $ echo a > a/a

Should fail - would corrupt dirstate:

  $ hg add a/a
  abort: file 'a' in dirstate clashes with 'a/a'
  [255]

  $ cd ..

Directory replaced with file:

  $ hg init c
  $ cd c
  $ mkdir a
  $ echo a > a/a
  $ hg commit -Ama
  adding a/a

  $ rm -r a
  $ echo a > a

Should fail - would corrupt dirstate:

  $ hg add a
  abort: directory 'a' already in dirstate
  [255]

  $ cd ..

Directory replaced with file:

  $ hg init d
  $ cd d
  $ mkdir b
  $ mkdir b/c
  $ echo a > b/c/d
  $ hg commit -Ama
  adding b/c/d
  $ rm -r b
  $ echo a > b

Should fail - would corrupt dirstate:

  $ hg add b
  abort: directory 'b' already in dirstate
  [255]


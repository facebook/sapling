  $ hg init a
  $ cd a
  $ echo a > a
  $ hg add -n
  adding a
  $ hg st
  ? a
  $ hg add
  adding a
  $ hg st
  A a
  $ hg forget a
  $ hg add
  adding a
  $ hg st
  A a

  $ echo b > b
  $ hg add -n b
  $ hg st
  A a
  ? b
  $ hg add b
  $ hg st
  A a
  A b

should fail

  $ hg add b
  b already tracked!
  $ hg st
  A a
  A b

  $ echo foo > con.xml
  $ hg --config ui.portablefilenames=jump add con.xml
  abort: ui.portablefilenames value is invalid ('jump')
  [255]
  $ hg --config ui.portablefilenames=abort add con.xml
  abort: filename contains 'con', which is reserved on Windows: 'con.xml'
  [255]
  $ hg st
  A a
  A b
  ? con.xml
  $ hg add con.xml
  warning: filename contains 'con', which is reserved on Windows: 'con.xml'
  $ hg st
  A a
  A b
  A con.xml
  $ echo bla > 'hello:world'
  $ hg --config ui.portablefilenames=abort add
  adding hello:world
  abort: filename contains ':', which is reserved on Windows: 'hello:world'
  [255]
  $ hg st
  A a
  A b
  A con.xml
  ? hello:world
  $ hg --config ui.portablefilenames=ignore add
  adding hello:world
  $ hg st
  A a
  A b
  A con.xml
  A hello:world

  $ hg ci -m 0 --traceback

should fail

  $ hg add a
  a already tracked!

  $ echo aa > a
  $ hg ci -m 1
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo aaa > a
  $ hg ci -m 2
  created new head

  $ hg merge
  merging a
  warning: conflicts during merge.
  merging a failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg st
  M a
  ? a.orig

should fail

  $ hg add a
  a already tracked!
  $ hg st
  M a
  ? a.orig
  $ hg resolve -m a
  $ hg ci -m merge

Issue683: peculiarity with hg revert of an removed then added file

  $ hg forget a
  $ hg add a
  $ hg st
  ? a.orig
  $ hg rm a
  $ hg st
  R a
  ? a.orig
  $ echo a > a
  $ hg add a
  $ hg st
  M a
  ? a.orig

  $ hg add c && echo "unexpected addition of missing file"
  c: No such file or directory
  [1]
  $ echo c > c
  $ hg add d c && echo "unexpected addition of missing file"
  d: No such file or directory
  [1]
  $ hg st
  M a
  A c
  ? a.orig


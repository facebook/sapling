#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.nativecheckout=true
  $ newserver server

  $ newremoterepo repo

  $ touch a
  $ hg add a
  $ hg commit -m "Added a"

  $ touch main
  $ hg add main
  $ hg commit -m "Added main"
  $ hg checkout c2eda428b523117ba9bbdfbbef034bb4bc8fead9
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

'main' should be gone:

  $ ls
  a

  $ touch side1
  $ hg add side1
  $ hg commit -m "Added side1"
  $ touch side2
  $ hg add side2
  $ hg commit -m "Added side2"

  $ hg log
  commit:      91ebc10ed028
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added side2
  
  commit:      b932d7dbb1e1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added side1
  
  commit:      71a760306caf
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added main
  
  commit:      c2eda428b523
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added a
  

  $ hg heads
  commit:      91ebc10ed028
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added side2
  
  commit:      71a760306caf
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added main
  
  $ ls
  a
  side1
  side2

  $ hg goto -C 71a760306cafb582ff672db4d4beb9625f34022d
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved

  $ ls
  a
  main


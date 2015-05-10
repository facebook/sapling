  $ hg init

  $ touch a
  $ hg add a
  $ hg commit -m "Added a"

  $ touch main
  $ hg add main
  $ hg commit -m "Added main"
  $ hg checkout 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

'main' should be gone:

  $ ls
  a

  $ touch side1
  $ hg add side1
  $ hg commit -m "Added side1"
  created new head
  $ touch side2
  $ hg add side2
  $ hg commit -m "Added side2"

  $ hg log
  changeset:   3:91ebc10ed028
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added side2
  
  changeset:   2:b932d7dbb1e1
  parent:      0:c2eda428b523
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added side1
  
  changeset:   1:71a760306caf
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added main
  
  changeset:   0:c2eda428b523
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added a
  

  $ hg heads
  changeset:   3:91ebc10ed028
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added side2
  
  changeset:   1:71a760306caf
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added main
  
  $ ls
  a
  side1
  side2

  $ hg update --debug -C 1
  resolving manifests
   branchmerge: False, force: True, partial: False
   ancestor: 91ebc10ed028+, local: 91ebc10ed028+, remote: 71a760306caf
   side1: other deleted -> r
  removing side1
   side2: other deleted -> r
  removing side2
   main: remote created -> g
  getting main
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved

  $ ls
  a
  main


  $ hg init a
  $ cd a
  $ touch empty1
  $ hg add empty1
  $ hg commit -m 'add empty1'

  $ touch empty2
  $ hg add empty2
  $ hg commit -m 'add empty2'

  $ hg up -C 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ touch empty3
  $ hg add empty3
  $ hg commit -m 'add empty3'
  created new head

  $ hg heads
  changeset:   2:a1cb177e0d44
  tag:         tip
  parent:      0:1e1d9c4e5b64
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add empty3
  
  changeset:   1:097d2b0e17f6
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add empty2
  

  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

Before changeset 05257fd28591, we didn't notice the
empty file that came from rev 1:

  $ hg status
  M empty2
  $ hg commit -m merge
  $ hg manifest --debug tip
  b80de5d138758541c5f05265ad144ab9fa86d1db 644   empty1
  b80de5d138758541c5f05265ad144ab9fa86d1db 644   empty2
  b80de5d138758541c5f05265ad144ab9fa86d1db 644   empty3

  $ cd ..

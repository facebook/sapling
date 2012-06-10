  $ hg init a
  $ cd a
  $ echo 123 > a
  $ hg add a
  $ hg commit -m "a" -u a

  $ cd ..
  $ hg init b
  $ cd b
  $ echo 321 > b
  $ hg add b
  $ hg commit -m "b" -u b

  $ hg pull ../a
  pulling from ../a
  searching for changes
  abort: repository is unrelated
  [255]

  $ hg pull -f ../a
  pulling from ../a
  searching for changes
  warning: repository is unrelated
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ hg heads
  changeset:   1:9a79c33a9db3
  tag:         tip
  parent:      -1:000000000000
  user:        a
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  changeset:   0:01f8062b2de5
  user:        b
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  

  $ cd ..

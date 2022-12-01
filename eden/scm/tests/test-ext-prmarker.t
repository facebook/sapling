#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible

  $ enable github

Build up a non-github repo

  $ hg init repo
  $ cd repo
  $ echo a > a1
  $ hg ci -Am addfile
  adding a1

Confirm debugprmarker is not enabled

  $ hg debugprmarker
  unknown command 'debugprmarker'
  (use 'hg help' to get help)
  [255]

Enable prmarker and confirm it does not abort on a non-github repo

  $ enable prmarker
  $ hg debugprmarker

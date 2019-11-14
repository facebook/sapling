#require fsmonitor

  $ newrepo
  $ hg status

  $ touch x

At t0:

  $ hg status
  ? x

  $ touch y

  $ hg debugrefreshwatchmanclock
  abort: only automation can run this
  [255]

At t1:

  $ HGPLAIN=1 hg debugrefreshwatchmanclock
  updating watchman clock from '*' to '*' (glob)

Changes between last watchman clock (t0) and "debugrefreshwatchmanclock" (t1) are missed ("touch y")

  $ hg status
  ? x

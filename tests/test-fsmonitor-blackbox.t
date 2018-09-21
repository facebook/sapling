#require fsmonitor

  $ newrepo
  $ enable blackbox
  $ setconfig blackbox.track=fsmonitor_details
  $ hg status
  $ touch x
  $ hg status
  ? x
  $ touch 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25
  $ hg status
  ? 1
  ? 10
  ? 11
  ? 12
  ? 13
  ? 14
  ? 15
  ? 16
  ? 17
  ? 18
  ? 19
  ? 2
  ? 20
  ? 21
  ? 22
  ? 23
  ? 24
  ? 25
  ? 3
  ? 4
  ? 5
  ? 6
  ? 7
  ? 8
  ? 9
  ? x
  $ grep returned .hg/blackbox.log
  *> watchman returned ['x'] (glob)
  *> watchman returned [*] and 5 more entries (glob)

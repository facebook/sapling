#chg-compatible

  $ configure modern
  $ newserver server1
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS
  $ hg bookmark -ir $B master

  $ cd $TESTTMP
  $ clone server1 client1
  $ cd client1

"master" is not lagging after clone:

  $ hg doctor --config doctor.check-lag-threshold=1
  checking internal storage
  checking commit references

Cause "lag" by adding a commit:

  $ drawdag --cwd $TESTTMP/server1 << "EOS"
  > D
  > |
  > tip
  > EOS
  $ hg --cwd $TESTTMP/server1 bookmark -ir tip master -q

  $ drawdag << "EOS"
  > C
  > |
  > tip
  > EOS

  $ hg doctor --config doctor.check-lag-threshold=1
  checking internal storage
  checking commit references
  master might be lagging, running pull

The latest master is pulled:

  $ hg log -r master -T '{desc}\n'
  D

Test too many names:

  $ hg debugremotebookmark name1 .
  $ hg debugremotebookmark name2 .
  $ hg debugremotebookmark name3 .
  $ hg log -r 'all()' -T '{desc}: {remotenames}.\n'
  A: .
  B: debugremote/name1 debugremote/name2 debugremote/name3.
  C: .
  D: remote/master.

  $ hg doctor --config doctor.check-too-many-names-threshold=1
  checking internal storage
  checking commit references
  repo has too many (4) remote bookmarks
  (only 1 of them (master) are essential)
  only keep essential remote bookmarks (Yn)? y
  $ hg log -r 'all()' -T '{desc}: {remotenames}.\n'
  A: .
  B: .
  C: .
  D: remote/master.

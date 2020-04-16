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

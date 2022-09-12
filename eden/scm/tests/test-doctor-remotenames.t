#chg-compatible
#debugruntest-compatible
  $ setconfig devel.segmented-changelog-rev-compat=true
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
  checking irrelevant draft branches for the workspace 'user/test/default'

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
  checking irrelevant draft branches for the workspace 'user/test/default'

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
  checking irrelevant draft branches for the workspace 'user/test/default'
  $ hg log -r 'all()' -T '{desc}: {remotenames}.\n'
  A: .
  B: .
  C: .
  D: remote/master.

Test less relevant branches:

  $ cd $TESTTMP
  $ clone server1 client2
  $ cd client2
  $ hg log -Gr 'all()' -T '{desc} {remotenames}'
  @  D remote/master
  │
  o  B
  │
  o  A
  
  $ drawdag << 'EOS'
  > F3  G3 G4 # amend: G3 -> G4
  > |   | /
  > F2  G2
  > |   |
  > F1  G1
  > |  /
  > tip
  > EOS

  $ hg doctor
  checking internal storage
  checking commit references
  checking irrelevant draft branches for the workspace 'user/test/default'

Changing the author, F branch becomes "less relevant". G is okay as it has
local modifications.

  $ HGUSER='Foo <f@o.o>' hg doctor
  checking internal storage
  checking commit references
  checking irrelevant draft branches for the workspace 'user/test/default'
  1 branches (627e777a207b) look less relevant
  hide those branches (Yn)? y

  $ hg log -Gr 'all()' -T '{desc} {remotenames}'
  o  G4
  │
  o  G2
  │
  o  G1
  │
  @  D remote/master
  │
  o  B
  │
  o  A
  

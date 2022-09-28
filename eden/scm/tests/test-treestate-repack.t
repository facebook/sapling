#chg-compatible

Fsmonitor makes the size numbers less predicatable.

#require no-fsmonitor

  $ setconfig format.dirstate=2
  $ setconfig treestate.mingcage=0

Prepare: stabilize random filename so it becomes predictable

  $ setconfig treestate.repackfactor=3 treestate.minrepackthreshold=100
  $ umask 022

Make some commits

  $ newrepo
  $ touch -t 200001010000 a b c d e
  $ hg ci -m init -A a b c d e -q --debug 2>&1 | grep treestate
  treestate repack threshold set to 507
  $ hg debugtreestate
  dirstate v2 (using treestate/*, offset 169, 5 files tracked) (glob)
  $ echo 1 > a
  $ touch -t 200001010000 a
  $ hg ci -m modify
  $ hg debugtreestate
  dirstate v2 (using treestate/*, offset 300, 5 files tracked) (glob)

Repack makes the file smaller

  $ hg debugtreestate repack --debug
  created treestate/* (glob)
  $ hg debugtreestate
  dirstate v2 (using treestate/*, offset 88, 5 files tracked) (glob)

Auto repack happens when treestate exceeds size threshold

  $ for i in 12 1 12 1 12 1; do
  >   echo .
  >   echo $i > a
  >   touch -t 200001010000 a
  >   hg ci -m modify -q --debug 2>&1 | grep treestate
  > done
  .
  treestate repack threshold set to 657
  .
  .
  .
  .
  .
  created treestate/* (glob)
  removing old unreferenced treestate/* (glob)
  $ hg debugtreestate
  dirstate v2 (using treestate/*, offset 88, 5 files tracked) (glob)

Cleanup removes the leftover files

  $ touch .hg/treestate/00000000-0000-0000-0000-000000000005
  $ hg debugtreestate cleanup --debug
  removing old unreferenced treestate/00000000-0000-0000-0000-000000000005

Cleanup does not remove files that are not old enough

  $ touch .hg/treestate/00000000-0000-0000-0000-000000000007
  $ hg debugtreestate cleanup --debug --config treestate.mingcage=1000

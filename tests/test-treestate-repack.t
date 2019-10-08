Fsmonitor makes the size numbers less predicatable.

#require no-fsmonitor

  $ setconfig format.dirstate=2
  $ setconfig treestate.mingcage=0

Prepare: fake uuid.uuid4 so it becomes predictable

  $ cat > fakeuuid.py << EOF
  > import os, uuid
  > path = os.path.join(os.environ.get('TESTTMP'), 'uuid')
  > def uuid4():
  >     value = 0
  >     try:
  >         value = int(open(path).read())
  >     except Exception:
  >         pass
  >     open(path, 'w').write('%s' % (value + 1))
  >     return '00000000-0000-0000-0000-%012d' % value
  > uuid.uuid4 = uuid4
  > EOF
  $ setconfig extensions.uuid=$TESTTMP/fakeuuid.py debug.dirstate.delaywrite=2
  $ setconfig treestate.repackfactor=3 treestate.minrepackthreshold=100
  $ umask 022

Make some commits

  $ newrepo
  $ touch -t 200001010000 a b c d e
  $ hg ci -m init -A a b c d e -q --debug | grep treestate
  treestate repack threshold set to 507
  $ hg debugtreestate
  dirstate v2 (using treestate/00000000-0000-0000-0000-000000000000, offset 169, 5 files tracked)
  $ echo 1 > a
  $ touch -t 200001010000 a
  $ hg ci -m modify
  $ hg debugtreestate
  dirstate v2 (using treestate/00000000-0000-0000-0000-000000000000, offset 359, 5 files tracked)

Repack makes the file smaller

  $ hg debugtreestate repack --debug
  creating treestate/00000000-0000-0000-0000-000000000001
  $ hg debugtreestate
  dirstate v2 (using treestate/00000000-0000-0000-0000-000000000001, offset 88, 5 files tracked)

Auto repack happens when treestate exceeds size threshold

  $ for i in 12 1 12 1 12 1; do
  >   echo .
  >   echo $i > a
  >   touch -t 200001010000 a
  >   hg ci -m modify -q --debug | grep treestate
  > done
  .
  treestate repack threshold set to 441
  .
  .
  creating treestate/00000000-0000-0000-0000-000000000002
  removing old unreferenced treestate/00000000-0000-0000-0000-000000000000
  treestate repack threshold set to 657
  .
  .
  .
  creating treestate/00000000-0000-0000-0000-000000000003
  removing old unreferenced treestate/00000000-0000-0000-0000-000000000001
  $ hg debugtreestate
  dirstate v2 (using treestate/00000000-0000-0000-0000-000000000003, offset 88, 5 files tracked)

Cleanup removes the leftover files

  $ touch .hg/treestate/00000000-0000-0000-0000-000000000005
  $ hg debugtreestate cleanup --debug
  removing old unreferenced treestate/00000000-0000-0000-0000-000000000005

Cleanup does not remove files that are not old enough

  $ touch .hg/treestate/00000000-0000-0000-0000-000000000007
  $ hg debugtreestate cleanup --debug --config treestate.mingcage=1000

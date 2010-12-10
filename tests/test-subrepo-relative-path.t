Preparing the subrepository 'sub'

  $ hg init sub
  $ echo sub > sub/sub
  $ hg add -R sub
  adding sub/sub
  $ hg commit -R sub -m "sub import"

Preparing the 'main' repo which depends on the subrepo 'sub'

  $ hg init main
  $ echo main > main/main
  $ echo "sub = ../sub" > main/.hgsub
  $ hg clone sub main/sub
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg add -R main
  adding main/.hgsub
  adding main/main
  $ hg commit -R main -m "main import"
  committing subrepository sub

Cleaning both repositories, just as a clone -U

  $ hg up -C -R sub null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg up -C -R main null
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ rm -rf main/sub

Serving them both using hgweb

  $ printf '[paths]\n/main = main\nsub = sub\n' > webdir.conf
  $ hg serve --webdir-conf webdir.conf -a localhost -p $HGPORT \
  >    -A /dev/null -E /dev/null --pid-file hg.pid -d
  $ cat hg.pid >> $DAEMON_PIDS

Clone main from hgweb

  $ hg clone "http://localhost:$HGPORT/main" cloned
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 3 changes to 3 files
  updating to branch default
  pulling subrepo sub from http://localhost:$HGPORT/sub
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Checking cloned repo ids

  $ hg id -R cloned
  fdfeeb3e979e tip
  $ hg id -R cloned/sub
  863c1745b441 tip

subrepo debug for 'main' clone

  $ hg debugsub -R cloned
  path sub
   source   ../sub
   revision 863c1745b441bd97a8c4a096e87793073f4fb215

  $ "$TESTDIR/killdaemons.py"


Create repo with nested relative subrepos

  $ hg init r1
  $ hg init r1/sub
  $ echo sub = sub > r1/.hgsub
  $ hg add --cwd r1 .hgsub
  $ hg init r1/sub/subsub
  $ echo subsub = subsub > r1/sub/.hgsub
  $ hg add --cwd r1/sub .hgsub
  $ echo c1 > r1/sub/subsub/f
  $ hg add --cwd r1/sub/subsub f
  $ hg ci --cwd r1 -m0
  committing subrepository sub
  committing subrepository sub/subsub

Ensure correct relative paths are used when pulling

  $ hg init r2
  $ cd r2/
  $ hg pull -u ../r1
  pulling from ../r1
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files
  pulling subrepo sub from ../r1/sub
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files
  pulling subrepo sub/subsub from ../r1/sub/subsub
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..

Verify subrepo default paths were set correctly

  $ hg -R r2/sub paths
  default = $TESTTMP/r1/sub
  $ cat r2/sub/.hg/hgrc
  [paths]
  default = ../../r1/sub
  $ hg -R r2/sub/subsub paths
  default = $TESTTMP/r1/sub/subsub
  $ cat r2/sub/subsub/.hg/hgrc
  [paths]
  default = ../../../r1/sub/subsub

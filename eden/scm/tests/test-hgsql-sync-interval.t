#chg-compatible

Test hgsql tries to sync before entering the critical section

  $ . "$TESTDIR/hgsql/library.sh"
  $ setconfig extensions.treemanifest=!
  $ setconfig hgsql.verbose=1

  $ newrepo state1
  $ hg debugdrawdag << 'EOS'
  > B
  > |
  > A
  > EOS

  $ newrepo state2
  $ hg debugdrawdag << 'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS

  $ newrepo state3
  $ hg debugdrawdag << 'EOS'
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS

  $ newrepo state4
  $ hg debugdrawdag << 'EOS'
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS

  $ cd $TESTTMP
  $ initserver repo1 master
  $ initserver repo2 master
  $ initserver repo3 master
  $ initserver repo4 master

Repo1: 2 commits. Sync them to the database.

  $ cd $TESTTMP/repo1
  $ hg pull -r tip $TESTTMP/state1
  [hgsql] got lock after * seconds (read 1 rows) (glob)
  pulling from $TESTTMP/state1
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  adding remote bookmark A
  adding remote bookmark B
  new changesets 426bada5c675:112478962961
  [hgsql] held lock for * seconds (read 5 rows; write 10 rows) (glob)

Repo2: Emulate client push. Hold the lock for long.

  $ cd $TESTTMP/repo2
  $ hg  --config hooks.pretxnclose.dely='sleep 6' pull -r tip $TESTTMP/state2 &>out &
  $ disown

Repo 3: Emulate client push to sql, after repo2.

  $ sleep 2
  $ cd $TESTTMP/repo3
  $ hg  --config hooks.pretxnclose.dely='sleep 6' pull -r tip $TESTTMP/state3 &>out &
  $ disown

Emulate writing to another repo when the lock was held elsewhere.
Explaination:
- The first "getting 2 commits from database" is because repo4 is empty, and the database has A,B synced from repo1.
- The second "getting 1 commits from database" is because repo2 push completes.
- The third "getting 1 commits from database" is because repo3 push completes.

  $ cd $TESTTMP/repo4
  $ setconfig hgsql.syncinterval=0.1 hgsql.debugminsqllockwaittime=13
  $ hg pull -r tip $TESTTMP/state4
  [hgsql] getting 2 commits from database
  [hgsql] getting 1 commits from database
  [hgsql] getting 1 commits from database
  [hgsql] got lock after * seconds (read * rows) (glob)
  pulling from $TESTTMP/state4
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  adding remote bookmark E
  new changesets 9bc730a19041
  [hgsql] held lock for * seconds (read * rows; write 8 rows) (glob)

Make sure repo2 and repo3 log looks sane.

  $ cat $TESTTMP/repo2/out
  [hgsql] getting 2 commits from database
  [hgsql] got lock after * seconds (read 1 rows) (glob)
  pulling from $TESTTMP/state2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  adding remote bookmark C
  new changesets 26805aba1e60
  [hgsql] held lock for * seconds (read 9 rows; write 8 rows) (glob)

  $ cat $TESTTMP/repo3/out
  [hgsql] getting 2 commits from database
  [hgsql] got lock after * seconds (read 1 rows) (glob)
  [hgsql] getting 1 commits from database
  pulling from $TESTTMP/state3
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  adding remote bookmark D
  new changesets f585351a92f8
  [hgsql] held lock for * seconds (read 10 rows; write 8 rows) (glob)

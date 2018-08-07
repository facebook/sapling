  $ setconfig remotefilelog.cachepath=$TESTTMP/cache extensions.remotefilelog=

  $ newrepo
  $ echo remotefilelog >> .hg/requires
  $ drawdag <<'EOS'
  > E  # E/X=1 (renamed from Y)
  > |
  > D  # D/Y=3 (renamed from X)
  > |
  > B  # B/X=2
  > |
  > A  # A/X=1
  > EOS

  $ hg bundle --all $TESTTMP/bundle --traceback -q

  $ newrepo
  $ echo remotefilelog >> .hg/requires
  $ hg unbundle $TESTTMP/bundle
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 8 changes to 6 files
  new changesets 52f22a21f8db:bf8514b268e7
  (run 'hg update' to get a working copy)


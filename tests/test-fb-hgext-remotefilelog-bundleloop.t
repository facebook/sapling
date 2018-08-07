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
  transaction abort!
  rollback completed
  abort: circular node dependency
  [255]



  $ hg init test
  $ cd test
  $ echo a > a
  $ hg ci -Ama
  adding a
  $ cd ..
  $ hg clone test test2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd test2
  $ echo a >> a
  $ hg ci -mb
  $ req() {
  >     hg serve -p $HGPORT -d --pid-file=hg.pid -E errors.log
  >     cat hg.pid >> $DAEMON_PIDS
  >     hg --cwd ../test2 push http://localhost:$HGPORT/
  >     "$TESTDIR/killdaemons.py"
  >     echo % serve errors
  >     cat errors.log
  > }
  $ cd ../test

expect ssl error

  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  remote: ssl required
  % serve errors

expect authorization error

  $ echo '[web]' > .hg/hgrc
  $ echo 'push_ssl = false' >> .hg/hgrc
  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  abort: authorization failed
  % serve errors

expect authorization error: must have authorized user

  $ echo 'allow_push = unperson' >> .hg/hgrc
  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  abort: authorization failed
  % serve errors

expect success

  $ echo 'allow_push = *' >> .hg/hgrc
  $ echo '[hooks]' >> .hg/hgrc
  $ echo 'changegroup = python "$TESTDIR"/printenv.py changegroup 0' >> .hg/hgrc
  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: changegroup hook: HG_NODE=ba677d0156c1196c1a699fa53f390dcfc3ce3872 HG_SOURCE=serve HG_URL=remote:http:*:  (glob)
  % serve errors
  $ hg rollback
  repository tip rolled back to revision 0 (undo serve)

expect success, server lacks the httpheader capability

  $ CAP=httpheader
  $ . "$TESTDIR/notcapable"
  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: changegroup hook: HG_NODE=ba677d0156c1196c1a699fa53f390dcfc3ce3872 HG_SOURCE=serve HG_URL=remote:http:*:  (glob)
  % serve errors
  $ hg rollback
  repository tip rolled back to revision 0 (undo serve)

expect success, server lacks the unbundlehash capability

  $ CAP=unbundlehash
  $ . "$TESTDIR/notcapable"
  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: changegroup hook: HG_NODE=ba677d0156c1196c1a699fa53f390dcfc3ce3872 HG_SOURCE=serve HG_URL=remote:http:*:  (glob)
  % serve errors
  $ hg rollback
  repository tip rolled back to revision 0 (undo serve)

expect authorization error: all users denied

  $ echo '[web]' > .hg/hgrc
  $ echo 'push_ssl = false' >> .hg/hgrc
  $ echo 'deny_push = *' >> .hg/hgrc
  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  abort: authorization failed
  % serve errors

expect authorization error: some users denied, users must be authenticated

  $ echo 'deny_push = unperson' >> .hg/hgrc
  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  abort: authorization failed
  % serve errors

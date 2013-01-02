  $ "$TESTDIR/hghave" killdaemons || exit 80

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
  >     exitstatus=$?
  >     "$TESTDIR/killdaemons.py" $DAEMON_PIDS
  >     echo % serve errors
  >     cat errors.log
  >     return $exitstatus
  > }
  $ cd ../test

expect ssl error

  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  abort: HTTP Error 403: ssl required
  % serve errors
  [255]

expect authorization error

  $ echo '[web]' > .hg/hgrc
  $ echo 'push_ssl = false' >> .hg/hgrc
  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  abort: authorization failed
  % serve errors
  [255]

expect authorization error: must have authorized user

  $ echo 'allow_push = unperson' >> .hg/hgrc
  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  abort: authorization failed
  % serve errors
  [255]

expect success

  $ echo 'allow_push = *' >> .hg/hgrc
  $ echo '[hooks]' >> .hg/hgrc
  $ echo "changegroup = python \"$TESTDIR/printenv.py\" changegroup 0" >> .hg/hgrc
  $ echo "pushkey = python \"$TESTDIR/printenv.py\" pushkey 0" >> .hg/hgrc
  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: changegroup hook: HG_NODE=ba677d0156c1196c1a699fa53f390dcfc3ce3872 HG_SOURCE=serve HG_URL=remote:http:*: (glob)
  remote: pushkey hook: HG_KEY=ba677d0156c1196c1a699fa53f390dcfc3ce3872 HG_NAMESPACE=phases HG_NEW=0 HG_OLD=1 HG_RET=1
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
  remote: changegroup hook: HG_NODE=ba677d0156c1196c1a699fa53f390dcfc3ce3872 HG_SOURCE=serve HG_URL=remote:http:*: (glob)
  remote: pushkey hook: HG_KEY=ba677d0156c1196c1a699fa53f390dcfc3ce3872 HG_NAMESPACE=phases HG_NEW=0 HG_OLD=1 HG_RET=1
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
  remote: changegroup hook: HG_NODE=ba677d0156c1196c1a699fa53f390dcfc3ce3872 HG_SOURCE=serve HG_URL=remote:http:*: (glob)
  remote: pushkey hook: HG_KEY=ba677d0156c1196c1a699fa53f390dcfc3ce3872 HG_NAMESPACE=phases HG_NEW=0 HG_OLD=1 HG_RET=1
  % serve errors
  $ hg rollback
  repository tip rolled back to revision 0 (undo serve)

expect push success, phase change failure

  $ cat > .hg/hgrc <<EOF
  > [web]
  > push_ssl = false
  > allow_push = *
  > [hooks]
  > prepushkey = python "$TESTDIR/printenv.py" prepushkey 1
  > EOF
  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  remote: prepushkey hook: HG_KEY=ba677d0156c1196c1a699fa53f390dcfc3ce3872 HG_NAMESPACE=phases HG_NEW=0 HG_OLD=1
  updating ba677d0156c1 to public failed!
  % serve errors

expect phase change success

  $ echo "prepushkey = python \"$TESTDIR/printenv.py\" prepushkey 0" >> .hg/hgrc
  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  no changes found
  remote: prepushkey hook: HG_KEY=ba677d0156c1196c1a699fa53f390dcfc3ce3872 HG_NAMESPACE=phases HG_NEW=0 HG_OLD=1
  % serve errors
  [1]
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
  [255]

expect authorization error: some users denied, users must be authenticated

  $ echo 'deny_push = unperson' >> .hg/hgrc
  $ req
  pushing to http://localhost:$HGPORT/
  searching for changes
  abort: authorization failed
  % serve errors
  [255]

  $ cd ..

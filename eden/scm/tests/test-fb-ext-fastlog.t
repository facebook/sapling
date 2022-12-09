#chg-compatible
  $ configure modernclient

  $ . "$TESTDIR/library.sh"

# Start up translation service.

  $ hg debugpython -- "$TESTDIR/conduithttp.py" --port-file conduit.port --pid conduit.pid
  $ cat conduit.pid >> $DAEMON_PIDS
  $ CONDUIT_PORT=`cat conduit.port`
  $ cat > ~/.arcrc <<EOF
  > {
  >   "hosts": {
  >     "https://phabricator.intern.facebook.com/api/": {
  >       "user": "testuser",
  >       "oauth": "testtoken"
  >     }
  >  }
  > }
  > EOF

# Test fastlog

  $ newclientrepo master
  $ echo x > x
  $ hg commit -qAm x
  $ echo x >> x
  $ hg commit -Aqm xx
  $ hg push -q --to master --create
  $ cd ..

  $ newclientrepo shallow test:master_server
  $ echo x >> x
  $ hg commit -Aqm xx2
  $ cd ../master
  $ echo y >> y
  $ hg commit -Aqm yy2
  $ echo x >> x
  $ hg commit -Aqm xx2-fake-rebased
  $ echo y >> y
  $ hg commit -Aqm yy3
  $ hg push -q --to master
  $ cd ../shallow
  $ hg pull -q
  $ hg goto master -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)
  $ echo x > x
  $ hg commit -qAm xx3

Verfiy correct linkrev despite fastlog failures

Case 1: fastlog service calls fails or times out

  $ echo {} > .arcconfig
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fbscmquery=
  > [fastlog]
  > enabled=True
  > [fbscmquery]
  > reponame = basic
  > host = localhost:$CONDUIT_PORT
  > path = /intern/conduit/
  > [phabricator]
  > arcrc_host = https://phabricator.intern.facebook.com/api/
  > graphql_host = http://none_such.intern.facebook.com:$CONDUIT_PORT
  > default_timeout = 60
  > graphql_app_id = 1234
  > graphql_app_token = TOKEN123
  > EOF
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased public x
  0632994590a8 xx public x
  b292c1e3311f x public x

Case 2: fastlog returns empty results

  $ clearcache
  $ cat >> .hg/hgrc <<EOF
  > [phabricator]
  > graphql_host = http://localhost:$CONDUIT_PORT
  > EOF
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/set_log_response/7200df4e0acad9339167ac526b0054b1bab32dee/
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased public x
  0632994590a8 xx public x
  b292c1e3311f x public x

Case 3: fastlog returns a bad hash

  $ clearcache
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/set_log_response/7200df4e0acad9339167ac526b0054b1bab32dee/123456
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased public x
  0632994590a8 xx public x
  b292c1e3311f x public x

Fastlog succeeds and returns the correct results

  $ clearcache
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/set_log_response/7200df4e0acad9339167ac526b0054b1bab32dee/32e6611f6149e85f58def77ee0c22549bb6953a2
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased public x
  0632994590a8 xx public x
  b292c1e3311f x public x

Fastlog should never get called on draft commits

  $ clearcache
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/set_log_response/a5957b6bf0bdeb9b96368bddd2838004ad966b7d/crash
  $ hg log -f x > /dev/null
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s (?)

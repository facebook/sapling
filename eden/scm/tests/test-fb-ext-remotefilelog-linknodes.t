#chg-compatible
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ disable treemanifest
  $ setconfig remotefilelog.write-hgcache-to-indexedlog=False remotefilelog.write-local-to-indexedlog=False

# Tests for the complicated linknode logic in remotefilelog.py::ancestormap()

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

# Initialise repo

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > serverexpiration=-1
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)


# Rebase produces correct log -f linknodes

  $ cd shallow
  $ echo y > y
  $ hg commit -qAm y
  $ hg up 'desc(x)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo x >> x
  $ hg commit -qAm xx
  $ hg log -f x --template "{node|short}\n"
  0632994590a8
  b292c1e3311f

  $ hg rebase -d 'desc(y)'
  rebasing 0632994590a8 "xx"
  $ hg log -f x --template "{node|short}\n"
  81deab2073bc
  b292c1e3311f


# Rebase back, log -f still works

  $ hg rebase -d b292c1e3311fd0f13ae83b409caae4a6d1fb348c -r 'max(desc(xx))'
  rebasing 81deab2073bc "xx"
  $ hg log -f x --template "{node|short}\n"
  b3fca10fb42d
  b292c1e3311f

  $ hg rebase -d 'desc(y)' -r 'desc(xx)'
  note: not rebasing 0632994590a8 "xx" and its descendants as this would cause divergence
  already rebased 81deab2073bc "xx"
  rebasing b3fca10fb42d "xx"

  $ cd ..

# Reset repos
  $ clearcache

  $ rm -rf master
  $ rm -rf shallow
  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > serverexpiration=-1
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

# Rebase stack onto landed commit

  $ cd master
  $ echo x >> x
  $ hg commit -Aqm xx

  $ cd ../shallow
  $ echo x >> x
  $ hg commit -Aqm xx2
  $ echo y >> x
  $ hg commit -Aqm xxy

  $ hg pull -q
  $ hg rebase -d tip
  rebasing 4549721d828f "xx2"
  note: rebase of 4549721d828f created no changes to commit
  rebasing 5ef6d97e851c "xxy"
  $ hg log -f x --template '{node|short}\n'
  4ae8e31c85ef
  0632994590a8
  b292c1e3311f

  $ cd ..

# system cache has invalid linknode, but .hg/store/data has valid

  $ cd shallow
  $ hg debugstrip -r 0632994590a85631ac9ce1a256862a1683a3ce56 -q
  $ rm -rf .hg/store/data/*
  $ echo x >> x
  $ hg commit -Aqm xx_local
  $ hg log -f x --template '{node|short}\n'
  21847713771d
  b292c1e3311f

  $ cd ..
  $ rm -rf shallow

/* Local linknode is invalid; remote linknode is valid (formerly slow case) */

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  $ cd shallow
  $ echo x >> x
  $ hg commit -Aqm xx2
  $ cd ../master
  $ echo y >> y
  $ hg commit -Aqm yy2
  $ echo x >> x
  $ hg commit -Aqm xx2-fake-rebased
  $ echo y >> y
  $ hg commit -Aqm yy3
  $ cd ../shallow
  $ hg pull --config remotefilelog.debug=True
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg update tip -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)
  $ echo x > x
  $ hg commit -qAm xx3

# At this point, the linknode points to c1254e70bad1 instead of 32e6611f6149
  $ hg log -G -T '{node|short} {desc} {phase} {files}\n'
  @  a5957b6bf0bd xx3 draft x
  │
  o  7200df4e0aca yy3 draft y
  │
  o  32e6611f6149 xx2-fake-rebased draft x
  │
  o  01979f9404f8 yy2 draft y
  │
  │ o  c1254e70bad1 xx2 draft x
  ├─╯
  o  0632994590a8 xx draft x
  │
  o  b292c1e3311f x draft x
  

# Check the contents of the local blob for incorrect linknode
  $ hg debughistorypack .hg/store/packs/086af52f91c7a0e07b80504e918a6daffadd1d1b.histpack
  
  x
  Node          P1 Node       P2 Node       Link Node     Copy From
  d4a3ed9310e5  aee31534993a  000000000000  c1254e70bad1  

# Verify that we do a fetch on the first log (remote blob fetch for linkrev fix)
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased draft x
  0632994590a8 xx draft x
  b292c1e3311f x draft x
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob) (?)

# But not after that
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased draft x
  0632994590a8 xx draft x
  b292c1e3311f x draft x

# Check the contents of the remote blob for correct linknode
  $ hg debughistorypack $CACHEDIR/master/packs/*.histpack
  
  x
  Node          P1 Node       P2 Node       Link Node     Copy From
  aee31534993a  1406e7411862  000000000000  0632994590a8  
  1406e7411862  000000000000  000000000000  b292c1e3311f  
  
  y
  Node          P1 Node       P2 Node       Link Node     Copy From
  d04f7aab46ef  076f5e2225b3  000000000000  7200df4e0aca  
  076f5e2225b3  000000000000  000000000000  01979f9404f8  
  
  x
  Node          P1 Node       P2 Node       Link Node     Copy From
  1406e7411862  000000000000  000000000000  b292c1e3311f  

Test the same scenario as above but with fastlog enabled

  $ cd ..
  $ clearcache

  $ rm -rf master
  $ rm -rf shallow
  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > serverexpiration=-1
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ echo x >> x
  $ hg commit -Aqm xx
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  $ cd shallow
  $ echo x >> x
  $ hg commit -Aqm xx2
  $ cd ../master
  $ echo y >> y
  $ hg commit -Aqm yy2
  $ echo x >> x
  $ hg commit -Aqm xx2-fake-rebased
  $ echo y >> y
  $ hg commit -Aqm yy3
  $ cd ../shallow
  $ hg pull -q
  $ hg update tip -q
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
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s (?)
  32e6611f6149 xx2-fake-rebased draft x
  0632994590a8 xx draft x
  b292c1e3311f x draft x

Case 2: fastlog returns empty results

  $ clearcache
  $ cat >> .hg/hgrc <<EOF
  > [phabricator]
  > graphql_host = http://localhost:$CONDUIT_PORT
  > EOF
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/set_log_response/7200df4e0acad9339167ac526b0054b1bab32dee/
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased draft x
  0632994590a8 xx draft x
  b292c1e3311f x draft x

Case 3: fastlog returns a bad hash

  $ clearcache
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/set_log_response/7200df4e0acad9339167ac526b0054b1bab32dee/123456
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased draft x
  0632994590a8 xx draft x
  b292c1e3311f x draft x

Fastlog succeeds and returns the correct results

  $ clearcache
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/set_log_response/7200df4e0acad9339167ac526b0054b1bab32dee/32e6611f6149e85f58def77ee0c22549bb6953a2
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased draft x
  0632994590a8 xx draft x
  b292c1e3311f x draft x

Fastlog should never get called on draft commits

  $ clearcache
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/set_log_response/a5957b6bf0bdeb9b96368bddd2838004ad966b7d/crash
  $ hg log -f x > /dev/null
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s (?)

Test linknode fixup logging

  $ clearcache

Setup extension that logs ui.log linkrevfixup output on the stderr
  $ cat >> $TESTTMP/uilog.py <<EOF
  > from edenscm import extensions
  > from edenscm import ui as uimod
  > def uisetup(ui):
  >     extensions.wrapfunction(uimod.ui, 'log', mylog)
  > def mylog(orig, self, service, *msg, **opts):
  >     if service in ['linkrevfixup']:
  >         kwstr = ", ".join("%s=%s" % (k, v) for k, v in
  >                           sorted(opts.items()))
  >         msgstr = msg[0] % msg[1:]
  >         self.warn('%s: %s (%s)\n' % (service, msgstr, kwstr))
  >     return orig(self, service, *msg, **opts)
  > EOF
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/set_log_response/a5957b6bf0bdeb9b96368bddd2838004ad966b7d/12356
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > uilog=$TESTTMP/uilog.py
  > EOF

Silencing stdout because we are interested only in ui.log output
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n' > /dev/null
  linkrevfixup: adjusting linknode (filepath=x, fnode=d4a3ed9310e5bd9887e3bf779da5077efab28216, reponame=master, revs=a5957b6bf0bdeb9b96368bddd2838004ad966b7d, user=test)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s (?)

Fastlog fails
  $ curl -s -X PUT http://localhost:$CONDUIT_PORT/set_log_response/7200df4e0acad9339167ac526b0054b1bab32dee/crash
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n' > /dev/null
  linkrevfixup: adjusting linknode (filepath=x, fnode=d4a3ed9310e5bd9887e3bf779da5077efab28216, reponame=master, revs=a5957b6bf0bdeb9b96368bddd2838004ad966b7d, user=test)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s (?)

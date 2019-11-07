  $ setconfig extensions.treemanifest=!
TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=

# Tests for the complicated linknode logic in remotefilelog.py::ancestormap()

  $ . "$TESTDIR/library.sh"

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
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)


# Rebase produces correct log -f linknodes

  $ cd shallow
  $ echo y > y
  $ hg commit -qAm y
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo x >> x
  $ hg commit -qAm xx
  $ hg log -f x --template "{node|short}\n"
  0632994590a8
  b292c1e3311f

  $ hg rebase -d 1
  rebasing 0632994590a8 "xx" (tip)
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/0632994590a8-0bc786d8-rebase.hg (glob)
  $ hg log -f x --template "{node|short}\n"
  81deab2073bc
  b292c1e3311f


# Rebase back, log -f still works

  $ hg rebase -d 0 -r 2
  rebasing 81deab2073bc "xx" (tip)
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/81deab2073bc-80cb4fda-rebase.hg (glob)
  $ hg log -f x --template "{node|short}\n"
  b3fca10fb42d
  b292c1e3311f

  $ hg rebase -d 1 -r 2
  rebasing b3fca10fb42d "xx" (tip)
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/b3fca10fb42d-da73a0c7-rebase.hg (glob)

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
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

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
  note: rebase of 1:4549721d828f created no changes to commit
  rebasing 5ef6d97e851c "xxy"
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/4549721d828f-b084e33c-rebase.hg (glob)
  $ hg log -f x --template '{node|short}\n'
  4ae8e31c85ef
  0632994590a8
  b292c1e3311f

  $ cd ..

# system cache has invalid linknode, but .hg/store/data has valid

  $ cd shallow
  $ hg debugstrip -r 1 -q
  $ rm -rf .hg/store/data/*
  $ echo x >> x
  $ hg commit -Aqm xx_local
  $ hg log -f x --template '{rev}:{node|short}\n'
  1:21847713771d
  0:b292c1e3311f

  $ cd ..
  $ rm -rf shallow

/* Local linknode is invalid; remote linknode is valid (formerly slow case) */

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
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
  added 3 changesets with 0 changes to 0 files
  new changesets 01979f9404f8:7200df4e0aca
  $ hg update tip -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ echo x > x
  $ hg commit -qAm xx3

# At this point, the linknode points to c1254e70bad1 instead of 32e6611f6149
  $ hg log -G -T '{node|short} {desc} {phase} {files}\n'
  @  a5957b6bf0bd xx3 draft x
  |
  o  7200df4e0aca yy3 public y
  |
  o  32e6611f6149 xx2-fake-rebased public x
  |
  o  01979f9404f8 yy2 public y
  |
  | o  c1254e70bad1 xx2 draft x
  |/
  o  0632994590a8 xx public x
  |
  o  b292c1e3311f x public x
  

# Check the contents of the local blob for incorrect linknode
  $ hg debughistorypack .hg/store/packs/086af52f91c7a0e07b80504e918a6daffadd1d1b.histpack
  
  x
  Node          P1 Node       P2 Node       Link Node     Copy From
  d4a3ed9310e5  aee31534993a  000000000000  c1254e70bad1  

# Verify that we do a fetch on the first log (remote blob fetch for linkrev fix)
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased public x
  0632994590a8 xx public x
  b292c1e3311f x public x
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

# But not after that
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased public x
  0632994590a8 xx public x
  b292c1e3311f x public x

# Check the contents of the remote blob for correct linknode
  $ hg debughistorypack $CACHEDIR/master/packs/861804d685584478f9eaa52741800152484b3566.histpack
  
  x
  Node          P1 Node       P2 Node       Link Node     Copy From
  d4a3ed9310e5  aee31534993a  000000000000  32e6611f6149  
  aee31534993a  1406e7411862  000000000000  0632994590a8  
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
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
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
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ echo x > x
  $ hg commit -qAm xx3

Verfiy correct linkrev despite fastlog failures

Case 1: fastlog service calls fails or times out

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fbconduit=$TESTTMP/bad_conduit.py
  > [fastlog]
  > enabled=True
  > EOF
  $ cat > $TESTTMP/bad_conduit.py <<EOF
  > def call_conduit(*args, **kwargs):
  >   raise Exception('error')
  > def conduit_config(*args, **kwargs):
  >   return True
  > EOF
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased public x
  0632994590a8 xx public x
  b292c1e3311f x public x
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s

Case 2: fastlog returns empty results

  $ cat > $TESTTMP/bad_conduit.py <<EOF
  > def call_conduit(*args, **kwargs):
  >   return []
  > def conduit_config(*args, **kwargs):
  >   return True
  > EOF
  $ rm $TESTTMP/bad_conduit.pyc
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased public x
  0632994590a8 xx public x
  b292c1e3311f x public x

Case 3: fastlog returns a bad hash

  $ cat > $TESTTMP/bad_conduit.py <<EOF
  > def call_conduit(*args, **kwargs):
  >   return [{'hash': '123456'}]
  > def conduit_config(*args, **kwargs):
  >   return True
  > EOF
  $ rm $TESTTMP/bad_conduit.pyc
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased public x
  0632994590a8 xx public x
  b292c1e3311f x public x

Fastlog succeeds and returns the correct results

  $ cat > $TESTTMP/bad_conduit.py <<EOF
  > def call_conduit(*args, **kwargs):
  >   return [{'hash': '32e6611f6149e85f58def77ee0c22549bb6953a2'}]
  > def conduit_config(*args, **kwargs):
  >   return True
  > EOF
  $ rm $TESTTMP/bad_conduit.pyc
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n'
  a5957b6bf0bd xx3 draft x
  32e6611f6149 xx2-fake-rebased public x
  0632994590a8 xx public x
  b292c1e3311f x public x

Fastlog should never get called on draft commits

  $ cat > $TESTTMP/bad_conduit.py <<EOF
  > import sys
  > def call_conduit(*args, **kwargs):
  >   if kwargs['rev'].startswith('a5957b6bf0bd'):
  >     sys.exit(80)
  >   return [{'hash': '32e6611f6149e85f58def77ee0c22549bb6953a2'}]
  > def conduit_config(*args, **kwargs):
  >   return True
  > EOF
  $ rm $TESTTMP/bad_conduit.pyc
  $ hg log -f x > /dev/null

Test linknode fixup logging

  $ clearcache

Setup extension that logs ui.log linkrevfixup output on the stderr
  $ cat >> $TESTTMP/uilog.py <<EOF
  > from edenscm.mercurial import extensions
  > from edenscm.mercurial import ui as uimod
  > def uisetup(ui):
  >     extensions.wrapfunction(uimod.ui, 'log', mylog)
  > def mylog(orig, self, service, *msg, **opts):
  >     if service in ['linkrevfixup']:
  >         kwstr = ", ".join("%s=%s" % (k, v) for k, v in
  >                           sorted(opts.iteritems()))
  >         msgstr = msg[0] % msg[1:]
  >         self.warn('%s: %s (%s)\n' % (service, msgstr, kwstr))
  >     return orig(self, service, *msg, **opts)
  > def call_conduit(*args, **kwargs):
  >   return [{'hash': '123456'}]
  > def conduit_config(*args, **kwargs):
  >   return True
  > EOF
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > uilog=$TESTTMP/uilog.py
  > EOF

Silencing stdout because we are interested only in ui.log output
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n' > /dev/null
  linkrevfixup: adjusting linknode (filepath=x, fnode=d4a3ed9310e5bd9887e3bf779da5077efab28216, reponame=master, revs=a5957b6bf0bdeb9b96368bddd2838004ad966b7d, user=test)
  linkrevfixup: fastlog succeded (elapsed=*, filepath=x, fnode=d4a3ed9310e5bd9887e3bf779da5077efab28216, reponame=master, revs=a5957b6bf0bdeb9b96368bddd2838004ad966b7d, user=test) (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s

Fastlog fails
  $ cat > $TESTTMP/bad_conduit.py <<EOF
  > def call_conduit(*args, **kwargs):
  >   raise Exception('error')
  > def conduit_config(*args, **kwargs):
  >   return True
  > EOF
  $ rm $TESTTMP/bad_conduit.pyc
  $ hg log -f x -T '{node|short} {desc} {phase} {files}\n' > /dev/null
  linkrevfixup: adjusting linknode (filepath=x, fnode=d4a3ed9310e5bd9887e3bf779da5077efab28216, reponame=master, revs=a5957b6bf0bdeb9b96368bddd2838004ad966b7d, user=test)
  linkrevfixup: fastlog failed (error) (elapsed=*, filepath=x, fnode=d4a3ed9310e5bd9887e3bf779da5077efab28216, reponame=master, revs=a5957b6bf0bdeb9b96368bddd2838004ad966b7d, user=test) (glob)
  linkrevfixup: remotefilelog prefetching succeeded (elapsed=*, filepath=x, fnode=d4a3ed9310e5bd9887e3bf779da5077efab28216, reponame=master, revs=a5957b6bf0bdeb9b96368bddd2838004ad966b7d, user=test) (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s

#chg-compatible
#inprocess-hg-incompatible

#require unix-permissions no-root no-windows no-eden

  $ configure modernclient

Prepare

  $ newclientrepo a
  $ cd ..
  $ echo a > a/a
  $ sl -R a ci -A -m a
  adding a
  $ sl -R a push -q --to book --create

  $ newclientrepo b a_server book
  $ cd ..

Test that raising an exception in the release function doesn't cause the lock to choke

  $ cat > testlock.py << EOF
  > from sapling import error, registrar
  > 
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > def acquiretestlock(repo, releaseexc):
  >     def unlock():
  >         if releaseexc:
  >             raise error.Abort('expected release exception')
  >     l = repo._lock(repo.localvfs, 'testlock', False, unlock, None, 'test lock')
  >     return l
  > 
  > @command('testlockexc')
  > def testlockexc(ui, repo):
  >     testlock = acquiretestlock(repo, True)
  >     try:
  >         testlock.release()
  >     finally:
  >         try:
  >             testlock = acquiretestlock(repo, False)
  >         except error.LockHeld:
  >             raise error.Abort('lockfile on disk even after releasing!')
  >         testlock.release()
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > testlock=$TESTTMP/testlock.py
  > EOF

  $ sl -R b testlockexc
  abort: expected release exception
  [255]

One process waiting for another for a significant period of time (longer than the default threshold). Warning should be shown.

  $ cat > hooks.py << EOF
  > import time
  > def sleeplong(**x):
  >     open("sleeping", "w").close()
  >     time.sleep(2)
  > EOF
  $ echo b > b/b
  $ sl -R b ci -A -m b --config hooks.precommit="python:`pwd`/hooks.py:sleeplong" > stdout 2>stderr &
Wait until bg process has entered critical section.
  $ python << EOF
  > import os
  > import sys
  > import time
  > deadline = time.time() + 10
  > while not os.path.exists("sleeping"):
  >     if time.time() >= deadline:
  >         print("timed out waiting for precommit hook", file=sys.stderr)
  >         break
  >     time.sleep(0.01)
  > EOF
  $ LOG=repolock=warn sl -R b up -q --config ui.timeout.warn=0 . > preup-stdout 2>preup-stderr
  $ wait
  $ cat stderr
  $ cat preup-stdout
  $ grep repolock preup-stderr | head -1
   WARN repolock: lock contended name="wlock" contents="*" (glob)
  $ cat stdout
  adding b

One process waiting for another for short period of time. No warning.

  $ cat > hooks.py << EOF
  > import time
  > def sleepone(**x): time.sleep(1)
  > def sleephalf(**x): time.sleep(0.5)
  > EOF
  $ echo b > b/c
  $ sl -R b ci -A -m b --config hooks.precommit="python:`pwd`/hooks.py:sleepone" > stdout &
  $ sl -R b up -q --config hooks.pre-update="python:`pwd`/hooks.py:sleephalf" . \
  > > preup-stdout 2>preup-stderr
  $ wait
  $ cat preup-stdout
  $ cat preup-stderr
  $ cat stdout
  adding c

On processs waiting on another, warning after a long time.

  $ echo b > b/d
  $ sl -R b ci -A -m b --config hooks.precommit="python:`pwd`/hooks.py:sleepone" > stdout &
  $ sl -R b up -q --config hooks.pre-update="python:`pwd`/hooks.py:sleephalf" . \
  > --config ui.timeout.warn=250 \
  > > preup-stdout 2>preup-stderr
  $ wait
  $ cat preup-stdout
  $ cat preup-stderr
  $ cat stdout
  adding d

On processs waiting on another, warning disabled.

  $ echo b > b/e
  $ sl -R b ci -A -m b --config hooks.precommit="python:`pwd`/hooks.py:sleepone" > stdout &
  $ sl -R b up -q --config hooks.pre-update="python:`pwd`/hooks.py:sleephalf" . \
  > --config ui.timeout.warn=-1 \
  > > preup-stdout 2>preup-stderr
  $ wait
  $ cat preup-stdout
  $ cat preup-stderr
  $ cat stdout
  adding e

check we still print debug output

On processs waiting on another, warning after a long time (debug output on)

  $ echo b > b/f
  $ sl -R b ci -A -m b --config hooks.precommit="python:`pwd`/hooks.py:sleepone" > stdout &
  $ sl -R b up --config hooks.pre-update="python:`pwd`/hooks.py:sleephalf" . \
  > --config ui.timeout.warn=250 --debug\
  > > preup-stdout 2>preup-stderr
  $ wait
  $ cat preup-stdout
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat preup-stderr
  got lock after * seconds (glob) (?)
  calling hook: pre-update
  $ cat stdout
  adding f

On processs waiting on another, warning disabled, (debug output on)

  $ echo b > b/g
  $ sl -R b ci -A -m b --config hooks.precommit="python:`pwd`/hooks.py:sleepone" > stdout &
  $ sl -R b up --config hooks.pre-update="python:`pwd`/hooks.py:sleephalf" . \
  > --config ui.timeout.warn=-1 --debug\
  > > preup-stdout 2>preup-stderr
  $ wait
  $ cat preup-stdout
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat preup-stderr
  got lock after * seconds (glob) (?)
  calling hook: pre-update
  $ cat stdout
  adding g

Having an empty lock file
  $ cd a
  $ touch .sl/wlock
  $ sl backout # a command which always acquires a lock
  abort: please specify a revision to backout
  [255]

Non-symlink stale lock is removed automatically.

Having an empty undolog lock file
  $ mkdir .sl/undolog && touch .sl/undolog/lock
  $ sl debuglocks
  lock:          free
  wlock:         free
  undolog/lock:  free
  prefetchlock:  free
  infinitepushbackup.lock: free
  $ sl debuglocks --force-undolog-lock
  abort: forcing lock release no longer supported
  [255]
  $ sl debuglocks
  lock:          free
  wlock:         free
  undolog/lock:  free
  prefetchlock:  free
  infinitepushbackup.lock: free

  $ cd ..

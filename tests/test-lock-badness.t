#require unix-permissions no-root no-windows

Prepare

  $ hg init a
  $ echo a > a/a
  $ hg -R a ci -A -m a
  adding a

  $ hg clone a b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test that raising an exception in the release function doesn't cause the lock to choke

  $ cat > testlock.py << EOF
  > from mercurial import cmdutil, error, error
  > 
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > 
  > def acquiretestlock(repo, releaseexc):
  >     def unlock():
  >         if releaseexc:
  >             raise error.Abort('expected release exception')
  >     l = repo._lock(repo.vfs, 'testlock', False, unlock, None, 'test lock')
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

  $ hg -R b testlockexc
  abort: expected release exception
  [255]

One process waiting for another

  $ cat > hooks.py << EOF
  > import time
  > def sleepone(**x): time.sleep(1)
  > def sleephalf(**x): time.sleep(0.5)
  > EOF
  $ echo b > b/b
  $ hg -R b ci -A -m b --config hooks.precommit="python:`pwd`/hooks.py:sleepone" > stdout &
  $ hg -R b up -q --config hooks.pre-update="python:`pwd`/hooks.py:sleephalf" \
  > > preup 2>&1
  $ wait
  $ cat preup
  waiting for lock on working directory of b held by '*:*' (glob)
  got lock after * seconds (glob)
  $ cat stdout
  adding b

Pushing to a local read-only repo that can't be locked

  $ chmod 100 a/.hg/store

  $ hg -R b push a
  pushing to a
  searching for changes
  abort: could not lock repository a: Permission denied
  [255]

  $ chmod 700 a/.hg/store

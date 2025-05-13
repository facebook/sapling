
#require no-eden


  $ eagerepo
  $ newext buggylocking <<EOF
  > """A small extension that tests our developer warnings
  > """
  > 
  > from sapling import error, registrar, repair, util
  > 
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > @command('buggylocking', [], '')
  > def buggylocking(ui, repo):
  >     lo = repo.lock()
  >     wl = repo.wlock()
  >     wl.release()
  >     lo.release()
  > 
  > @command('buggytransaction', [], '')
  > def buggylocking(ui, repo):
  >     tr = repo.transaction('buggy')
  >     # make sure we rollback the transaction as we don't want to rely on the__del__
  >     tr.release()
  > 
  > @command('properlocking', [], '')
  > def properlocking(ui, repo):
  >     """check that reentrance is fine"""
  >     wl = repo.wlock()
  >     lo = repo.lock()
  >     tr = repo.transaction('proper')
  >     tr2 = repo.transaction('proper')
  >     lo2 = repo.lock()
  >     wl2 = repo.wlock()
  >     wl2.release()
  >     lo2.release()
  >     tr2.close()
  >     tr.close()
  >     lo.release()
  >     wl.release()
  > 
  > @command('nowaitlocking', [], '')
  > def nowaitlocking(ui, repo):
  >     lo = repo.lock()
  >     wl = repo.wlock(wait=False)
  >     wl.release()
  >     lo.release()
  > 
  > @command('no-wlock-write', [], '')
  > def nowlockwrite(ui, repo):
  >     with repo.vfs('branch', 'a'):
  >         pass
  > 
  > @command('no-lock-write', [], '')
  > def nolockwrite(ui, repo):
  >     with repo.svfs('fncache', 'a'):
  >         pass
  > 
  > @command('stripintr', [], '')
  > def stripintr(ui, repo):
  >     lo = repo.lock()
  >     tr = repo.transaction('foobar')
  >     try:
  >         repair.strip(repo.ui, repo, [repo['.'].node()])
  >     finally:
  >         lo.release()
  > @command('oldanddeprecated', [], '')
  > def oldanddeprecated(ui, repo):
  >     """test deprecation warning API"""
  >     def foobar(ui):
  >         ui.deprecwarn('foorbar is deprecated, go shopping', '42.1337')
  >     foobar(ui)
  > @command('nouiwarning', [], '')
  > def nouiwarning(ui, repo):
  >     util.nouideprecwarn('this is a test', '13.37')
  > @command('programmingerror', [], '')
  > def programmingerror(ui, repo):
  >     raise error.ProgrammingError('something went wrong', hint='try again')
  > EOF

  $ setconfig devel.all-warnings=1

  $ hg init lock-checker
  $ cd lock-checker
#if no-fsmonitor
  $ hg buggylocking
  devel-warn: "wlock" acquired after "lock" at: $TESTTMP/buggylocking.py:* (buggylocking) (glob)
  $ cat << EOF >> $HGRCPATH
  > [devel]
  > all=0
  > check-locks=1
  > EOF
  $ hg buggylocking
  devel-warn: "wlock" acquired after "lock" at: $TESTTMP/buggylocking.py:* (buggylocking) (glob)
  $ hg buggylocking --traceback 2>&1 | egrep '(devel-warn|buggylocking)'
  devel-warn: "wlock" acquired after "lock" at:
   * (glob) (?)
   * (glob) (?)
   * (glob) (?)
#endif
  $ hg properlocking
  $ hg nowaitlocking

Writing without lock (also uses bare repo.vfs)

  $ hg no-wlock-write
  devel-warn: use of bare vfs instead of localvfs or sharedvfs at: $TESTTMP/buggylocking.py:* (nowlockwrite) (glob)
  devel-warn: write with no wlock: "branch" at: $TESTTMP/buggylocking.py:* (nowlockwrite) (glob)

  $ hg no-lock-write
  devel-warn: write with no lock: "fncache" at: * (glob)

Stripping from a transaction

  $ echo a > a
  $ hg add a
  $ hg commit -m a
  $ hg stripintr 2>&1 | egrep -v '^(\*\*|  )'
  Traceback (most recent call last):
  *ProgrammingError: cannot strip from inside a transaction (glob)

  $ hg oldanddeprecated
  devel-warn: foorbar is deprecated, go shopping
  (compatibility will be dropped after Mercurial-42.1337, update your code.) at: $TESTTMP/buggylocking.py:* (oldanddeprecated) (glob)

  $ hg oldanddeprecated --traceback 2>&1 | egrep '(buggylocking|devel-warn)'
  devel-warn: foorbar is deprecated, go shopping
   * (glob)
   * (glob) (?)
   * (glob) (?)

#if no-chg normal-layout no-fsmonitor
  $ hg blackbox --no-timestamp --no-sid --pattern '{"legacy_log":{"service":"develwarn"}}' | grep develwarn
  [legacy][develwarn] devel-warn: "wlock" acquired after "lock" at: $TESTTMP/buggylocking.py:12 (buggylocking)
  [legacy][develwarn] devel-warn: "wlock" acquired after "lock" at: $TESTTMP/buggylocking.py:12 (buggylocking)
  [legacy][develwarn] devel-warn: "wlock" acquired after "lock" at:
  [legacy][develwarn] devel-warn: use of bare vfs instead of localvfs or sharedvfs at: $TESTTMP/buggylocking.py:47 (nowlockwrite)
  [legacy][develwarn] devel-warn: write with no wlock: "branch" at: $TESTTMP/buggylocking.py:47 (nowlockwrite)
  [legacy][develwarn] devel-warn: write with no lock: "fncache" at: * (glob)
  [legacy][develwarn] devel-warn: foorbar is deprecated, go shopping
  [legacy][develwarn] devel-warn: foorbar is deprecated, go shopping
#endif

Test programming error failure:

  $ hg buggytransaction 2>&1 | egrep -v '^  '
  ** * has crashed: (glob)
  ** ProgrammingError: transaction requires locking
  Traceback (most recent call last):
  *ProgrammingError: transaction requires locking (glob)

  $ hg programmingerror 2>&1 | egrep -v '^  '
  ** * has crashed: (glob)
  ** ProgrammingError: something went wrong
  ** (try again)
  Traceback (most recent call last):
  *ProgrammingError: something went wrong (glob)

#if bash no-chg
Old style deprecation warning

  $ hg nouiwarning
  $TESTTMP/buggylocking.py:*: DeprecationWarning: this is a test (glob)
  (compatibility will be dropped after Mercurial-13.37, update your code.)
    util.nouideprecwarn('this is a test', '13.37')

(disabled outside of test run)

  $ HGEMITWARNINGS= hg nouiwarning
#endif


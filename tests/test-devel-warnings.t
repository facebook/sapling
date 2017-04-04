
  $ cat << EOF > buggylocking.py
  > """A small extension that tests our developer warnings
  > """
  > 
  > from mercurial import cmdutil, repair, util
  > 
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
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
  > EOF

  $ cat << EOF >> $HGRCPATH
  > [extensions]
  > buggylocking=$TESTTMP/buggylocking.py
  > mock=$TESTDIR/mockblackbox.py
  > blackbox=
  > [devel]
  > all-warnings=1
  > EOF

  $ hg init lock-checker
  $ cd lock-checker
  $ hg buggylocking
  devel-warn: "wlock" acquired after "lock" at: $TESTTMP/buggylocking.py:* (buggylocking) (glob)
  $ cat << EOF >> $HGRCPATH
  > [devel]
  > all=0
  > check-locks=1
  > EOF
  $ hg buggylocking
  devel-warn: "wlock" acquired after "lock" at: $TESTTMP/buggylocking.py:* (buggylocking) (glob)
  $ hg buggylocking --traceback
  devel-warn: "wlock" acquired after "lock" at:
   */hg:* in * (glob)
   */mercurial/dispatch.py:* in run (glob)
   */mercurial/dispatch.py:* in dispatch (glob)
   */mercurial/dispatch.py:* in _runcatch (glob)
   */mercurial/dispatch.py:* in callcatch (glob)
   */mercurial/scmutil.py* in callcatch (glob)
   */mercurial/dispatch.py:* in _runcatchfunc (glob)
   */mercurial/dispatch.py:* in _dispatch (glob)
   */mercurial/dispatch.py:* in runcommand (glob)
   */mercurial/dispatch.py:* in _runcommand (glob)
   */mercurial/dispatch.py:* in <lambda> (glob)
   */mercurial/util.py:* in check (glob)
   $TESTTMP/buggylocking.py:* in buggylocking (glob)
  $ hg properlocking
  $ hg nowaitlocking

  $ echo a > a
  $ hg add a
  $ hg commit -m a
  $ hg stripintr 2>&1 | egrep -v '^(\*\*|  )'
  saved backup bundle to $TESTTMP/lock-checker/.hg/strip-backup/*-backup.hg (glob)
  Traceback (most recent call last):
  mercurial.error.ProgrammingError: cannot strip from inside a transaction

  $ hg oldanddeprecated
  devel-warn: foorbar is deprecated, go shopping
  (compatibility will be dropped after Mercurial-42.1337, update your code.) at: $TESTTMP/buggylocking.py:* (oldanddeprecated) (glob)

  $ hg oldanddeprecated --traceback
  devel-warn: foorbar is deprecated, go shopping
  (compatibility will be dropped after Mercurial-42.1337, update your code.) at:
   */hg:* in <module> (glob)
   */mercurial/dispatch.py:* in run (glob)
   */mercurial/dispatch.py:* in dispatch (glob)
   */mercurial/dispatch.py:* in _runcatch (glob)
   */mercurial/dispatch.py:* in callcatch (glob)
   */mercurial/scmutil.py* in callcatch (glob)
   */mercurial/dispatch.py:* in _runcatchfunc (glob)
   */mercurial/dispatch.py:* in _dispatch (glob)
   */mercurial/dispatch.py:* in runcommand (glob)
   */mercurial/dispatch.py:* in _runcommand (glob)
   */mercurial/dispatch.py:* in <lambda> (glob)
   */mercurial/util.py:* in check (glob)
   $TESTTMP/buggylocking.py:* in oldanddeprecated (glob)
  $ hg blackbox -l 7
  1970/01/01 00:00:00 bob @cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b (5000)> oldanddeprecated
  1970/01/01 00:00:00 bob @cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b (5000)> devel-warn: foorbar is deprecated, go shopping
  (compatibility will be dropped after Mercurial-42.1337, update your code.) at: $TESTTMP/buggylocking.py:* (oldanddeprecated) (glob)
  1970/01/01 00:00:00 bob @cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b (5000)> oldanddeprecated exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b (5000)> oldanddeprecated --traceback
  1970/01/01 00:00:00 bob @cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b (5000)> devel-warn: foorbar is deprecated, go shopping
  (compatibility will be dropped after Mercurial-42.1337, update your code.) at:
   */hg:* in <module> (glob)
   */mercurial/dispatch.py:* in run (glob)
   */mercurial/dispatch.py:* in dispatch (glob)
   */mercurial/dispatch.py:* in _runcatch (glob)
   */mercurial/dispatch.py:* in callcatch (glob)
   */mercurial/scmutil.py* in callcatch (glob)
   */mercurial/dispatch.py:* in _runcatchfunc (glob)
   */mercurial/dispatch.py:* in _dispatch (glob)
   */mercurial/dispatch.py:* in runcommand (glob)
   */mercurial/dispatch.py:* in _runcommand (glob)
   */mercurial/dispatch.py:* in <lambda> (glob)
   */mercurial/util.py:* in check (glob)
   $TESTTMP/buggylocking.py:* in oldanddeprecated (glob)
  1970/01/01 00:00:00 bob @cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b (5000)> oldanddeprecated --traceback exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b (5000)> blackbox -l 7

Test programming error failure:

  $ hg buggytransaction 2>&1 | egrep -v '^  '
  ** Unknown exception encountered with possibly-broken third-party extension buggylocking
  ** which supports versions unknown of Mercurial.
  ** Please disable buggylocking and try your action again.
  ** If that fixes the bug please report it to the extension author.
  ** Python * (glob)
  ** Mercurial Distributed SCM (*) (glob)
  ** Extensions loaded: * (glob)
  Traceback (most recent call last):
  mercurial.error.ProgrammingError: transaction requires locking

Old style deprecation warning

  $ hg nouiwarning
  $TESTTMP/buggylocking.py:61: DeprecationWarning: this is a test
  (compatibility will be dropped after Mercurial-13.37, update your code.)
    util.nouideprecwarn('this is a test', '13.37')

(disabled outside of test run)

  $ HGEMITWARNINGS= hg nouiwarning

  $ cd ..

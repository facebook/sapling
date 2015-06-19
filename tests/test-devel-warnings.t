
  $ cat << EOF > buggylocking.py
  > """A small extension that acquire locks in the wrong order
  > """
  > 
  > from mercurial import cmdutil, repair, revset
  > 
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > 
  > @command('buggylocking', [], '')
  > def buggylocking(ui, repo):
  >     tr = repo.transaction('buggy')
  >     lo = repo.lock()
  >     wl = repo.wlock()
  >     wl.release()
  >     lo.release()
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
  > 
  > def oldstylerevset(repo, subset, x):
  >     return list(subset)
  > 
  > revset.symbols['oldstyle'] = oldstylerevset
  > EOF

  $ cat << EOF >> $HGRCPATH
  > [extensions]
  > buggylocking=$TESTTMP/buggylocking.py
  > [devel]
  > all-warnings=1
  > EOF

  $ hg init lock-checker
  $ cd lock-checker
  $ hg buggylocking
  devel-warn: transaction with no lock at: $TESTTMP/buggylocking.py:11 (buggylocking)
  devel-warn: "wlock" acquired after "lock" at: $TESTTMP/buggylocking.py:13 (buggylocking)
  $ cat << EOF >> $HGRCPATH
  > [devel]
  > all=0
  > check-locks=1
  > EOF
  $ hg buggylocking
  devel-warn: transaction with no lock at: $TESTTMP/buggylocking.py:11 (buggylocking)
  devel-warn: "wlock" acquired after "lock" at: $TESTTMP/buggylocking.py:13 (buggylocking)
  $ hg buggylocking --traceback
  devel-warn: transaction with no lock at:
   */hg:* in * (glob)
   */mercurial/dispatch.py:* in run (glob)
   */mercurial/dispatch.py:* in dispatch (glob)
   */mercurial/dispatch.py:* in _runcatch (glob)
   */mercurial/dispatch.py:* in _dispatch (glob)
   */mercurial/dispatch.py:* in runcommand (glob)
   */mercurial/dispatch.py:* in _runcommand (glob)
   */mercurial/dispatch.py:* in checkargs (glob)
   */mercurial/dispatch.py:* in <lambda> (glob)
   */mercurial/util.py:* in check (glob)
   $TESTTMP/buggylocking.py:* in buggylocking (glob)
  devel-warn: "wlock" acquired after "lock" at:
   */hg:* in * (glob)
   */mercurial/dispatch.py:* in run (glob)
   */mercurial/dispatch.py:* in dispatch (glob)
   */mercurial/dispatch.py:* in _runcatch (glob)
   */mercurial/dispatch.py:* in _dispatch (glob)
   */mercurial/dispatch.py:* in runcommand (glob)
   */mercurial/dispatch.py:* in _runcommand (glob)
   */mercurial/dispatch.py:* in checkargs (glob)
   */mercurial/dispatch.py:* in <lambda> (glob)
   */mercurial/util.py:* in check (glob)
   $TESTTMP/buggylocking.py:* in buggylocking (glob)
  $ hg properlocking
  $ hg nowaitlocking

  $ echo a > a
  $ hg add a
  $ hg commit -m a
  $ hg stripintr
  saved backup bundle to $TESTTMP/lock-checker/.hg/strip-backup/cb9a9f314b8b-cc5ccb0b-backup.hg (glob)
  abort: programming error: cannot strip from inside a transaction
  (contact your extension maintainer)
  [255]

  $ hg log -r "oldstyle()" -T '{rev}\n'
  devel-warn: revset "oldstyle" use list instead of smartset, (upgrade your code) at: */mercurial/revset.py:* (mfunc) (glob)
  0

  $ cd ..

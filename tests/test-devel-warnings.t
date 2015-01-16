
  $ cat << EOF > buggylocking.py
  > """A small extension that acquire locks in the wrong order
  > """
  > 
  > from mercurial import cmdutil
  > 
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > 
  > @command('buggylocking', [], '')
  > def buggylocking(ui, repo):
  >     lo = repo.lock()
  >     wl = repo.wlock()
  > EOF

  $ cat << EOF >> $HGRCPATH
  > [extensions]
  > buggylocking=$TESTTMP/buggylocking.py
  > [devel]
  > all=1
  > EOF

  $ hg init lock-checker
  $ cd lock-checker
  $ hg buggylocking
  "lock" taken before "wlock"
  $ cat << EOF >> $HGRCPATH
  > [devel]
  > all=0
  > check-locks=1
  > EOF
  $ hg buggylocking
  "lock" taken before "wlock"
  $ hg buggylocking --traceback
  "lock" taken before "wlock"
   at:
   */hg:* in <module> (glob)
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
  $ cd ..

Dummy extension simulating long running command
  $ cat > sleepext.py <<EOF
  > import time
  > import itertools
  > 
  > from mercurial import cmdutil
  > from mercurial.i18n import _
  > 
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > 
  > @command('sleep', [], _('TIME'), norepo=True)
  > def sleep(ui, sleeptime="1", **opts):
  > 
  >     for _i in itertools.repeat(None, int(sleeptime)):
  >         time.sleep(1)
  > 
  >     ui.warn("%s second(s) passed\n" % sleeptime)
  > EOF

Set up repository
  $ hg init repo
  $ cd repo
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > sleepext = ../sleepext.py
  > EOF

Test ctrl-c
  $ timeout -s 2 1 hg sleep 2
  interrupted!
  [124]

  $ cat >> $HGRCPATH << EOF
  > nointerrupt = $TESTDIR/../nointerrupt.py
  > [alias]
  > slumber = sleep
  > [nointerrupt]
  > attend-sleep = True
  > attend-update = True
  > EOF

  $ timeout -s 2 1 hg sleep 2
  interrupted!
  [124]

  $ cat >> $HGRCPATH << EOF
  > interactiveonly = False
  > EOF

  $ timeout -s 2 1 hg sleep 2
  ==========================
  Interrupting Mercurial may leave your repo in a bad state.
  If you really want to interrupt your current command, press
  CTRL-C again.
  ==========================
  2 second(s) passed
  [124]

  $ timeout -s 2 1 hg slum 2
  ==========================
  Interrupting Mercurial may leave your repo in a bad state.
  If you really want to interrupt your current command, press
  CTRL-C again.
  ==========================
  2 second(s) passed
  [124]

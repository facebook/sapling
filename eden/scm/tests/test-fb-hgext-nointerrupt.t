#chg-compatible

Dummy extension simulating long running command
  $ newext sleepext <<EOF
  > import time
  > import itertools
  > 
  > from edenscm.mercurial import registrar
  > from edenscm.mercurial.i18n import _
  > 
  > cmdtable = {}
  > command = registrar.command(cmdtable)
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

#if osx
  $ TIMEOUT=gtimeout
#else
  $ TIMEOUT=timeout
#endif

  $ hash $TIMEOUT 2>/dev/null
  > HASHSTATUS=$?
  > if [ $HASHSTATUS -ne 0 ] ; then
  >     echo "skipped: missing feature: $TIMEOUT"
  >     exit 80
  > fi

Test ctrl-c
  $ $TIMEOUT -s 2 1 hg sleep 2
  interrupted!
  [124]

  $ readglobalconfig <<EOF
  > [extensions]
  > nointerrupt=
  > [alias]
  > slum = sleep
  > [nointerrupt]
  > attend-sleep = True
  > attend-update = True
  > EOF

  $ $TIMEOUT -s 2 1 hg sleep 2
  interrupted!
  [124]

  $ readglobalconfig <<EOF
  > [nointerrupt]
  > interactiveonly = False
  > EOF

  $ $TIMEOUT -s 2 1 hg sleep 2
  ==========================
  Interrupting Mercurial may leave your repo in a bad state.
  If you really want to interrupt your current command, press
  CTRL-C again.
  ==========================
  2 second(s) passed
  [124]

  $ $TIMEOUT -s 2 1 hg slum 2
  ==========================
  Interrupting Mercurial may leave your repo in a bad state.
  If you really want to interrupt your current command, press
  CTRL-C again.
  ==========================
  2 second(s) passed
  [124]

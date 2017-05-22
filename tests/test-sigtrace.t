  $ PYTHONPATH=$TESTDIR/../:$PYTHONPATH
  $ export PYTHONPATH

  $ cat >> $TESTTMP/signal.py << EOF
  > from mercurial import registrar
  > import os, signal
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('signal', norepo=True)
  > def signalcommand(ui, *pats, **kwds):
  >     os.kill(os.getpid(), getattr(signal, 'SIG' + pats[0]))
  > EOF

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > sigtrace=$TESTDIR/../hgext3rd/sigtrace.py
  > signal=$TESTTMP/signal.py
  > [sigtrace]
  > pathformat=$TESTTMP/dump-%(pid)s-%(time)s.log
  > EOF

Test the default SIGUSR1 signal

  $ hg signal USR1
  $ ls $TESTTMP/dump-*.log
  $TESTTMP/dump-*-*.log (glob)
  $ grep Thread $TESTTMP/dump-*.log | head -n 1
  Thread *: (glob)
  $ rm $TESTTMP/dump-*.log

Test the signal config option

  $ echo 'signal=USR2' >> $HGRCPATH
  $ hg signal USR2
  $ ls $TESTTMP/dump-*.log
  $TESTTMP/dump-*-*.log (glob)
  $ grep Thread $TESTTMP/dump-*.log | head -n 1
  Thread *: (glob)
  $ rm $TESTTMP/dump-*.log

  $ echo 'signal=INVALIDSIGNAL' >> $HGRCPATH
  $ hg signal USR1 || false
  * (glob)
  [1]
  $ ls $TESTTMP/dump-*.log || false
  ls: * (glob)
  [1]

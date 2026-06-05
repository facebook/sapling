#inprocess-hg-incompatible

  $ eagerepo
  $ newext signal <<EOF
  > from sapling import registrar
  > import os, signal
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('signal', norepo=True)
  > def signalcommand(ui, *pats, **kwds):
  >     os.kill(os.getpid(), getattr(signal, 'SIG' + pats[0]))
  > EOF

  $ enable sigtrace
  $ setconfig sigtrace.pathformat="$TESTTMP/dump-%(pid)s-%(time)s.log"

Test the default SIGUSR1 signal

  $ sl signal USR1 2>&1 | tail -1
  * written to $TESTTMP/dump-*.log (glob)
  $ ls $TESTTMP/dump-*.log
  $TESTTMP/dump-*-*.log (glob)
  $ grep Thread $TESTTMP/dump-*.log | head -n 1
  Thread *: (glob)
  $ rm $TESTTMP/dump-*.log

Test the signal config option

  $ echo 'signal=USR2' >> $HGRCPATH
  $ echo 'memsignal=USR1' >> $HGRCPATH
  $ sl signal USR2  2>&1 | tail -1
  * written to $TESTTMP/dump-*.log (glob)
  $ ls $TESTTMP/dump-*.log
  $TESTTMP/dump-*-*.log (glob)
  $ grep Thread $TESTTMP/dump-*.log | head -n 1
  Thread *: (glob)
  $ rm $TESTTMP/dump-*.log

  $ echo 'signal=INVALIDSIGNAL' >> $HGRCPATH
  $ echo 'memsignal=INVALIDSIGNAL' >> $HGRCPATH
  $ sl signal USR1 || false
  [1]
  $ ls $TESTTMP/dump-*.log || false
  ls: * (glob)
  [1]

Test the interval config option

  $ newrepo
  $ setconfig sigtrace.interval=1
  $ sl dbsh -c 'import time; time.sleep(2)'
  $ ls .sl/sigtrace/
  pid-*-debugshell (glob)

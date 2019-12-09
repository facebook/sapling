#chg-compatible

Test UI worker interaction

  $ cat > t.py <<EOF
  > from __future__ import absolute_import, print_function
  > import time
  > from edenscm.mercurial import (
  >     error,
  >     registrar,
  >     ui as uimod,
  >     worker,
  > )
  > def abort(ui, args):
  >     if args[0] == 0:
  >         # by first worker for test stability
  >         raise error.Abort('known exception')
  >     return runme(ui, [])
  > def exc(ui, args):
  >     if args[0] == 0:
  >         # by first worker for test stability
  >         raise Exception('unknown exception')
  >     return runme(ui, [])
  > def runme(ui, args):
  >     for arg in args:
  >         ui.status('run\n')
  >         yield 1, 0, arg
  >     time.sleep(0.1) # easier to trigger killworkers code path
  > functable = {
  >     'abort': abort,
  >     'exc': exc,
  >     'runme': runme,
  > }
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command(b'test', [], 'hg test [COST] [FUNC]')
  > def t(ui, repo, cost=1.0, func='runme'):
  >     cost = float(cost)
  >     func = functable[func]
  >     ui.status('start\n')
  >     runs = worker.worker(ui, cost, func, (ui,), range(8))
  >     for n, size, i in runs:
  >         pass
  >     ui.status('done\n')
  > EOF
  $ abspath=`pwd`/t.py
  $ hg init

Run tests with worker enable by forcing a heigh cost

  $ hg --config "extensions.t=$abspath" test 100000.0
  start
  run
  run
  run
  run
  run
  run
  run
  run
  done

Run tests without worker by forcing a low cost

  $ hg --config "extensions.t=$abspath" test 0.0000001
  start
  run
  run
  run
  run
  run
  run
  run
  run
  done

#if no-windows

Known exception should be caught, but printed if --traceback is enabled

  $ hg --config "extensions.t=$abspath" --config 'worker.numcpus=8' \
  > test 100000.0 abort 2>&1
  start
  abort: known exception
  [255]

  $ hg --config "extensions.t=$abspath" --config 'worker.numcpus=8' \
  > test 100000.0 abort --traceback 2>&1 | egrep '^(SystemExit|Abort)'
  Abort: known exception
  SystemExit: 255

Traceback must be printed for unknown exceptions

  $ hg --config "extensions.t=$abspath" --config 'worker.numcpus=8' \
  > test 100000.0 exc 2>&1 | grep '^Exception'
  Exception: unknown exception

Workers should not do cleanups in all cases

  $ cat > $TESTTMP/detectcleanup.py <<EOF
  > from __future__ import absolute_import
  > import atexit
  > import os
  > import time
  > oldfork = os.fork
  > count = 0
  > parentpid = os.getpid()
  > def delayedfork():
  >     global count
  >     count += 1
  >     pid = oldfork()
  >     # make it easier to test SIGTERM hitting other workers when they have
  >     # not set up error handling yet.
  >     if count > 1 and pid == 0:
  >         time.sleep(0.1)
  >     return pid
  > os.fork = delayedfork
  > def cleanup():
  >     if os.getpid() != parentpid:
  >         os.write(1, 'should never happen\n')
  > atexit.register(cleanup)
  > EOF

  $ hg --config "extensions.t=$abspath" --config worker.numcpus=8 --config \
  > "extensions.d=$TESTTMP/detectcleanup.py" test 100000 abort
  start
  abort: known exception
  [255]

#endif

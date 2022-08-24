#chg-compatible
#debugruntest-compatible

Test UI worker interaction

  $ cat > t.py <<EOF
  > from __future__ import absolute_import, print_function
  > import time
  > from edenscm import (
  >     error,
  >     registrar,
  >     ui as uimod,
  >     worker,
  > )
  > def abort(ui, args):
  >     if len(args) > 0 and args[0] == 0:
  >         # by first worker for test stability
  >         raise error.Abort('known exception')
  >     return runme(ui, [])
  > def exc(ui, args):
  >     if len(args) > 0 and args[0] == 0:
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
  > @command('test', [], 'hg test [COST] [FUNC]')
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
  > test 100000.0 abort --traceback 2>&1 | egrep '^(SystemExit|(edenscm.error.)?Abort)'
  *Abort: known exception (glob)
  *Abort: known exception (glob)

Traceback must be printed for unknown exceptions

  $ hg --config "extensions.t=$abspath" --config 'worker.numcpus=8' \
  > test 100000.0 exc 2>&1 | grep '^Exception' | sort
  Exception in thread Thread-*: (glob)
  Exception: unknown exception
  Exception: unknown exception

#endif

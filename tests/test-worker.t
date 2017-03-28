Test UI worker interaction

  $ cat > t.py <<EOF
  > from __future__ import absolute_import, print_function
  > from mercurial import (
  >     cmdutil,
  >     ui as uimod,
  >     worker,
  > )
  > def runme(ui, args):
  >     for arg in args:
  >         ui.status('run\n')
  >         yield 1, arg
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > @command('test', [], 'hg test [COST]')
  > def t(ui, repo, cost=1.0):
  >     cost = float(cost)
  >     ui.status('start\n')
  >     runs = worker.worker(ui, cost, runme, (ui,), range(8))
  >     for n, i in runs:
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

#inprocess-hg-incompatible

  $ eagerepo
  $ newext crash <<EOF
  > from sapling import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('crash', [])
  > def crash(ui, repo):
  >     raise Exception('crash')
  > EOF
  $ enable errorredirect
  $ setconfig extensions.mock="$TESTDIR/mockblackbox.py"

Test errorredirect will respect original behavior by default
  $ sl init
  $ sl crash 2>&1 | grep -o 'crashed'
  crashed

Test the errorredirect script will override stack trace output
  $ sl crash --config errorredirect.script='echo overridden-message'
  overridden-message
  [255]

If the script returns non-zero, print the trace
  $ sl crash --config errorredirect.script='echo It works && exit 1' 2>&1 | grep '^[IT]'
  It works
  Traceback (most recent call last):

  $ printf '#!%sbin/sh\necho It works && false' '/' > a.sh
  $ chmod +x $TESTTMP/a.sh
  $ PATH=$TESTTMP:$PATH sl crash --config errorredirect.script=a.sh 2>&1 | grep '^[IT]'
  It works
  Traceback (most recent call last):

If the script is terminated by SIGTERM (Ctrl+C), do not print the trace
  $ sl crash --config errorredirect.script='echo It works && kill -TERM $$' 2>&1
  It works
  [255]

  $ printf '#!%sbin/sh\necho It works && kill -TERM $$' '/' > a.sh
  $ chmod +x $TESTTMP/a.sh
  $ PATH=$TESTTMP:$PATH sl crash --config errorredirect.script=a.sh 2>&1
  It works
  [255]

If the script cannot be executed (not found in PATH), print the trace
  $ sl crash --config errorredirect.script="$TESTTMP/SCRIPT-DOES-NOT-EXIST" 2>&1 | grep '^[IT]'
  Traceback (most recent call last):

Traces are logged in blackbox
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > blackbox=
  > [blackbox]
  > track = command, command_exception
  > logsource = 1
  > EOF

  $ sl crash --config errorredirect.script='echo Works'
  Works
  [255]
  $ sl blackbox --pattern '{"legacy_log":{"service":"command_exception"}}' 2>&1 | head -n 1
  * [legacy][command_exception] ** has crashed: (glob)

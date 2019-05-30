  $ cat > $TESTTMP/crash.py << EOF
  > from edenscm.mercurial import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('crash', [])
  > def crash(ui, repo):
  >     raise 'crash'
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > errorredirect=
  > crash=$TESTTMP/crash.py
  > mock=$TESTDIR/mockblackbox.py
  > EOF

Test errorredirect will respect original behavior by default
  $ hg init
  $ hg crash 2>&1 | grep -o 'crashed'
  crashed

Test the errorredirect script will override stack trace output
  $ hg crash --config errorredirect.script='echo overridden-message'
  overridden-message
  [255]

If the script returns non-zero, print the trace
  $ hg crash --config errorredirect.script='echo It works && exit 1' 2>&1 | grep '^[IT]'
  It works
  Traceback (most recent call last):
  TypeError: exceptions must be old-style classes or derived from BaseException, not str

  $ printf '#!%sbin/sh\necho It works && false' '/' > a.sh
  $ chmod +x $TESTTMP/a.sh
  $ PATH=$TESTTMP:$PATH hg crash --config errorredirect.script=a.sh 2>&1 | grep '^[IT]'
  It works
  Traceback (most recent call last):
  TypeError: exceptions must be old-style classes or derived from BaseException, not str

If the script is terminated by SIGTERM (Ctrl+C), do not print the trace
  $ hg crash --config errorredirect.script='echo It works && kill -TERM $$' 2>&1
  It works
  [255]

  $ printf '#!%sbin/sh\necho It works && kill -TERM $$' '/' > a.sh
  $ chmod +x $TESTTMP/a.sh
  $ PATH=$TESTTMP:$PATH hg crash --config errorredirect.script=a.sh 2>&1
  It works
  [255]

If the script cannot be executed (not found in PATH), print the trace
  $ hash SCRIPT-DOES-NOT-EXIST 2>/dev/null && exit 80
  [1]
  $ hg crash --config errorredirect.script='SCRIPT-DOES-NOT-EXIST' 2>&1 | grep '^[IT]'
  Traceback (most recent call last):
  TypeError: exceptions must be old-style classes or derived from BaseException, not str

Traces are logged in blackbox
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > blackbox=
  > [blackbox]
  > track = command, command_exception
  > logsource = 1
  > EOF

  $ hg blackbox -l 2
  1970/01/01 00:00:00 bob @(unknown) (5000) [command]> blackbox -l 2
  $ hg crash --config errorredirect.script='echo Works'
  Works
  [255]
  $ hg blackbox -l 12 | grep '\[command'
  1970/01/01 00:00:00 bob @(unknown) (5000) [command]> blackbox -l 2
  1970/01/01 00:00:00 bob @(unknown) (5000) [command]> crash --config 'errorredirect.script=echo Works'
  1970/01/01 00:00:00 bob @(unknown) (5000) [command_exception]> ** Mercurial Distributed SCM (version *) has crashed: (glob)
  1970/01/01 00:00:00 bob @(unknown) (5000) [command]> blackbox -l 12

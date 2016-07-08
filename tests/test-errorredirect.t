  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/errorredirect.py $TESTTMP
  $ cat > $TESTTMP/crash.py << EOF
  > from mercurial import cmdutil
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > @command('crash', [])
  > def crash(ui, repo):
  >     raise 'crash'
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > errorredirect=$TESTTMP/errorredirect.py
  > crash=$TESTTMP/crash.py
  > EOF

Test errorredirect will respect original behavior by default
  $ hg init
  $ hg crash 2>&1 | grep -o 'Unknown exception encountered'
  Unknown exception encountered

Test the errorredirect script will override stack trace output
  $ hg crash --config errorredirect.script='echo overridden-message'
  overridden-message
  [255]

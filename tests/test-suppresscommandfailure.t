Test if suppresscommandfailure correctly suppresses a commandexception warning.

  $ hg init
  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/suppresscommandfailure.py $TESTTMP
  $ cat > $TESTTMP/crash.py << EOF
  > from mercurial import cmdutil
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > @command('crash', [])
  > def crash(ui, repo):
  >     raise ValueError('crash')
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > suppresscommandfailure=$TESTTMP/suppresscommandfailure.py
  > crash=$TESTTMP/crash.py
  > EOF

Test the extension will override the normal warning output
  $ hg crash
  [1]

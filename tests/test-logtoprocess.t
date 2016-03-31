Test if logtoprocess correctly captures command-related log calls.

  $ hg init
  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/logtoprocess.py $TESTTMP
  $ cat > $TESTTMP/foocommand.py << EOF
  > from mercurial import cmdutil
  > from time import sleep
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > @command('foo', [])
  > def foo(ui, repo):
  >     ui.log('foo', 'a message: %(bar)s\n', bar='spam')
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > logtoprocess=$TESTTMP/logtoprocess.py
  > foocommand=$TESTTMP/foocommand.py
  > [logtoprocess]
  > command=echo 'logtoprocess command output:';
  >     echo "\$MSG1";
  >     echo "\$MSG2"
  > commandfinish=echo 'logtoprocess commandfinish output:';
  >     echo "\$MSG1";
  >     echo "\$MSG2";
  >     echo "\$MSG3"
  > foo=echo 'logtoprocess foo output:';
  >     echo "\$MSG1";
  >     echo "\$OPT_BAR"
  > EOF

Running a command triggers both a ui.log('command') and a
ui.log('commandfinish') call. The foo command also uses ui.log:

  $ hg foo
  logtoprocess command output:
  foo
  
  foo
  logtoprocess foo output:
  a message: spam
  
  spam
  logtoprocess commandfinish output:
  foo exited 0 after * seconds (glob)
  
  foo
  0

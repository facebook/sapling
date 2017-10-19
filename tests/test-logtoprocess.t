#require no-windows

ATTENTION: logtoprocess runs commands asynchronously. Be sure to append "| cat"
to hg commands, to wait for the output, if you want to test its output.
Otherwise the test will be flaky.

Test if logtoprocess correctly captures command-related log calls.

  $ hg init
  $ cat > $TESTTMP/foocommand.py << EOF
  > from __future__ import absolute_import
  > from mercurial import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > configtable = {}
  > configitem = registrar.configitem(configtable)
  > configitem('logtoprocess', 'foo',
  >     default=None,
  > )
  > @command(b'foo', [])
  > def foo(ui, repo):
  >     ui.log('foo', 'a message: %(bar)s\n', bar='spam')
  > EOF
  $ cp $HGRCPATH $HGRCPATH.bak
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > logtoprocess=
  > foocommand=$TESTTMP/foocommand.py
  > [logtoprocess]
  > command=echo 'logtoprocess command output:';
  >     echo "\$EVENT";
  >     echo "\$MSG1";
  >     echo "\$MSG2"
  > commandfinish=echo 'logtoprocess commandfinish output:';
  >     echo "\$EVENT";
  >     echo "\$MSG1";
  >     echo "\$MSG2";
  >     echo "\$MSG3"
  > foo=echo 'logtoprocess foo output:';
  >     echo "\$EVENT";
  >     echo "\$MSG1";
  >     echo "\$OPT_BAR"
  > EOF

Running a command triggers both a ui.log('command') and a
ui.log('commandfinish') call. The foo command also uses ui.log.

Use sort to avoid ordering issues between the various processes we spawn:
  $ hg foo | cat | sort
  
  
  
   (chg !)
  0
  a message: spam
  command
  command (chg !)
  commandfinish
  foo
  foo
  foo
  foo
  foo exited 0 after * seconds (glob)
  logtoprocess command output:
  logtoprocess command output: (chg !)
  logtoprocess commandfinish output:
  logtoprocess foo output:
  serve --cmdserver chgunix * (glob) (chg !)
  serve --cmdserver chgunix * (glob) (chg !)
  spam

Confirm that logging blocked time catches stdio properly:
  $ cp $HGRCPATH.bak $HGRCPATH
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > logtoprocess=
  > pager=
  > [logtoprocess]
  > uiblocked=echo "\$EVENT stdio \$OPT_STDIO_BLOCKED ms command \$OPT_COMMAND_DURATION ms"
  > [ui]
  > logblockedtimes=True
  > EOF

  $ hg log | cat
  uiblocked stdio [0-9]+.[0-9]* ms command [0-9]+.[0-9]* ms (re)

  $ extpath=$(dirname $TESTDIR)
  $ cp $extpath/githelp.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > githelp=$TESTTMP/githelp.py
  > EOF

  $ hg init repo
  $ cd repo

githelp on a single command should succeed
  $ hg githelp -- commit
  hg commit

githelp on a command with options should succeed
  $ hg githelp -- commit -pm "abc"
  hg record -m 'abc'

githelp on a command with standalone unrecognized option should succeed with warning
  $ hg githelp -- commit -p -v
  ignoring unknown option -v
  hg record

githelp on a command with unrecognized option packed with other options should fail with error
  $ hg githelp -- commit -pv
  abort: unknown option v packed with other options
  Please try passing the option as it's own flag: -v
  
  If this is a valid git command, please log a task for the source_control oncall.
  
  [255]
